// SPDX-License-Identifier: MIT
use super::shaders::{Refusal, Variant};
use super::*;
use crate::geometry_registry_abi::RSF_ANOMALY_PASS_MAIN;
use crate::motion_device_abi::{
    RSF_MOTION_CORE, RSF_MOTION_FOLIAGE, RSF_MOTION_PIXEL, RSF_MOTION_TESS,
};

const SOFTWARE_SKINNED: u8 = 3;

fn foliage_family(family: u8) -> bool {
    matches!(family, FOLIAGE_TREE | FOLIAGE_DETAIL)
}

impl DrawFacts {
    fn class(&self, staged: bool) -> usize {
        if staged {
            8
        } else if self.detail {
            6
        } else if !self.in_visual {
            7
        } else if self.verdict == STATIC_WORLD {
            0
        } else {
            match self.kind {
                RIGID => 1,
                HARDWARE_SKINNED => 2,
                SINGLE_BONE => 3,
                SOFTWARE_SKINNED => 4,
                _ => 5,
            }
        }
    }

    fn context_known(&self) -> bool {
        self.view <= 1 && self.domain <= 1
    }

    fn main_pass(&self) -> bool {
        i32::from(self.pass) == RSF_ANOMALY_PASS_MAIN
    }
}

fn engine_matrix(matrix: &[f32; 16]) -> Mat4 {
    let mut result = MaybeUninit::uninit();
    rsf_anomaly_motion_from_engine(matrix, &mut result);
    unsafe { result.assume_init() }
}

fn camera(p: &Platform) -> Option<CameraSample> {
    let mut view = [0.; 16];
    let mut projection = [0.; 16];
    unsafe { (p.camera)(&mut view, &mut projection) }.then(|| CameraSample {
        view: engine_matrix(&view),
        projection: engine_matrix(&projection),
    })
}

fn world(p: &Platform) -> Option<[f32; 16]> {
    let mut matrix = [0.; 16];
    unsafe { (p.world)(&mut matrix) }.then_some(matrix)
}

fn read_constant<T>(p: &Platform, var: &ConstantRef, dest: &mut T) -> bool {
    // Vars removed by the stock compiler leave their zero-initialized words.
    !var.present
        || unsafe {
            (p.read_constant)(
                var.buffer,
                var.buffer_size,
                var.offset,
                ptr::from_mut(dest).cast(),
                var.size.min(size_of::<T>() as u32),
            )
        }
}

fn read_foliage(p: &Platform, layout: &FoliageLayout, out: &mut FoliageConstants) -> bool {
    read_constant(p, &layout.consts, &mut out.consts)
        && read_constant(p, &layout.wave, &mut out.wave)
        && read_constant(p, &layout.wind, &mut out.wind)
}

impl Render {
    fn poison_batch(&mut self, p: &Platform, layout: &FoliageLayout) {
        let mut rows = MaybeUninit::<foliage::Rows>::uninit();
        foliage::rsf_anomaly_foliage_poison(&mut rows);
        let array = &layout.array;
        // The completed flush already uploaded the live rows; the engine fills
        // its next batch before the next flush. Poison identifies the rest.
        if unsafe {
            (p.write_constant)(
                array.buffer,
                array.buffer_size,
                array.offset,
                rows.as_ptr().cast(),
                size_of::<foliage::Rows>() as u32,
            )
        } {
            self.counters.foliage_poisoned += 1;
        } else {
            self.counters.foliage_poison_failed += 1;
        }
    }

    fn record_history(&mut self, p: &Platform, facts: &DrawFacts) -> History {
        let mut h = History::default();
        if !facts.main_pass()
            || !facts.context_known()
            || self.history.views[lane(facts.view)].current.is_none()
        {
            return h;
        }
        let view = lane(facts.view);
        let domain = lane(facts.domain);
        h.sample = camera(p);
        if let Some(sample) = &h.sample {
            h.conflict = matches!(
                self.history.observe(view, domain, sample),
                CameraStatus::Conflict
            );
            self.counters.camera_conflicts += u64::from(h.conflict);
        }
        if !facts.capture.is_null() {
            let mut snapshot = MaybeUninit::uninit();
            if unsafe { (p.snapshot)(facts.capture, snapshot.as_mut_ptr()) } {
                let facts = unsafe { snapshot.assume_init() };
                let input = SnapshotInput {
                    draw: DrawSnapshotView {
                        world: facts.world,
                        bones: PaletteView {
                            data: facts.bones,
                            count: facts.bone_count,
                        },
                    },
                    palette_layout: facts.palette_layout,
                    deformation_constants: ptr::null(),
                    deformation_count: 0,
                    deformation_layout: 0,
                    detail: ptr::null(),
                    domain: facts.domain,
                    kind: facts.kind,
                    has_deformation: false,
                };
                let snapshot = unsafe { Snapshot::borrow(&input) };
                h.previous = self
                    .history
                    .record(view, facts.key, input.domain, snapshot)
                    .map_or_else(DrawSnapshotView::default, Snapshot::draw_view);
                h.stale =
                    unsafe { rsf_anomaly_motion_stale_palette(&h.previous, &input.draw, 0.001) };
                self.counters.snapshots += 1;
            }
        }
        h
    }

    fn foliage_bytes(
        &mut self,
        stage: &Stage,
        draw: &Draw<'_>,
        vertex: &Variant,
        bytes: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>,
    ) -> HistoryReason {
        rsf_anomaly_motion_pack_foliage_invalid(bytes);
        if !stage.foliage.load(Relaxed) || !vertex.foliage.usable {
            return HistoryReason::Foliage;
        }
        let facts = draw.facts;
        let p = stage.platform;
        let layout = &vertex.foliage;
        let known = facts.context_known();
        let camera = known
            .then(|| {
                self.history
                    .previous_camera(lane(facts.view), lane(facts.domain))
            })
            .flatten()
            .map_or(ptr::null(), ptr::from_ref);
        let active = known && foliage::rsf_anomaly_foliage_active(Some(&self.foliage), facts.view);
        let [_, _, previous_u, previous_v] = draw.state.jitter;
        let mut current = FoliageConstants::default();
        if vertex.family == FOLIAGE_TREE {
            let mut xform = [0.; 12];
            let constants_ok = facts.in_visual
                && facts.kind == TREE
                && facts.visual != 0
                && read_constant(p, &layout.xform, &mut xform)
                && read_foliage(p, layout, &mut current);
            let current = TreeRecord {
                xform,
                constants: current,
                lod: facts.tree_lod,
            };
            let previous = if constants_ok && active {
                foliage::rsf_anomaly_foliage_tree(
                    Some(&mut self.foliage),
                    facts.view,
                    facts.visual,
                    &current,
                )
            } else {
                ptr::null()
            };
            let input = TreeDrawInput {
                family: vertex.family,
                in_visual: facts.in_visual,
                kind: facts.kind,
                verdict: facts.verdict,
                context: known,
                camera_conflict: draw.history.conflict,
                previous_camera: camera,
                constants_ok,
                current,
                previous,
                previous_jitter_u: previous_u,
                previous_jitter_v: previous_v,
            };
            let clip = match unsafe { decide_tree(&input) } {
                Ok(clip) => clip,
                Err(reason) => return reason,
            };
            // decide_tree requires the previous record, borrowed from this view.
            let previous = unsafe { &*previous };
            if rsf_anomaly_motion_pack_tree(&clip, &previous.xform, &previous.constants, bytes) {
                return HistoryReason::Valid;
            }
            return HistoryReason::FoliageNoConstants;
        }
        let matrix = world(p);
        let world_identity = matrix.is_some_and(|m| {
            m.map(f32::to_bits)
                == [
                    1.0_f32, 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
                ]
                .map(f32::to_bits)
        });
        let mut rows = [[0.; 16]; FOLIAGE_MAX_GROUPS];
        let constants_ok = facts.detail
            && read_foliage(p, layout, &mut current)
            && read_constant(p, &layout.array, &mut rows);
        let mut remap = foliage::DetailRemap::empty();
        if constants_ok {
            // Wave 0 and wave 1 can share the same shader permutation.
            let route = current.wave[..3]
                .iter()
                .fold(draw.stock.vs as u64, |route, w| {
                    (route ^ u64::from(w.to_bits())).wrapping_mul(1_099_511_628_211)
                });
            if active {
                let mut output = MaybeUninit::uninit();
                foliage::rsf_anomaly_foliage_detail(
                    Some(&mut self.foliage),
                    facts.view,
                    route,
                    &current,
                    &rows,
                    &mut output,
                );
                remap = unsafe { output.assume_init() };
            } else {
                remap.live = foliage::rsf_anomaly_foliage_count(&rows);
            }
            self.poison_batch(p, layout);
            self.counters.detail_items += u64::from(remap.live);
            self.counters.detail_items_matched += u64::from(remap.matched);
            self.counters.detail_items_unmatched += u64::from(remap.unmatched);
            self.counters.detail_conflicts += u64::from(remap.conflicts);
        }
        let input = DetailDrawInput {
            family: vertex.family,
            detail_window: facts.detail,
            context: known,
            camera_conflict: draw.history.conflict,
            previous_camera: camera,
            constants_ok,
            world_readable: matrix.is_some(),
            world_identity,
            route_found: remap.route_found,
            live_groups: remap.live,
            previous_jitter_u: previous_u,
            previous_jitter_v: previous_v,
        };
        let clip = match unsafe { decide_detail(&input) } {
            Ok(clip) => clip,
            Err(reason) => return reason,
        };
        if remap.matched == 0 {
            return HistoryReason::FoliageNoPrevious;
        }
        if unsafe {
            rsf_anomaly_motion_pack_detail(
                &clip,
                &remap.previous,
                remap.items.as_ptr(),
                remap.items.len(),
                remap.live,
                bytes,
            )
        } {
            HistoryReason::Valid
        } else {
            HistoryReason::FoliageNoConstants
        }
    }

    fn upload_core(&mut self, draw: &Draw<'_>, vertex: &Variant, tessellated: bool) -> bool {
        let facts = draw.facts;
        let known = facts.context_known();
        let previous_camera = if known {
            self.history
                .previous_camera(lane(facts.view), lane(facts.domain))
                .map_or(ptr::null(), ptr::from_ref)
        } else {
            ptr::null()
        };
        let input = CoreDrawInput {
            family: vertex.family,
            skin: vertex.skin,
            in_visual: facts.in_visual,
            verdict: facts.verdict,
            kind: facts.kind,
            context: known,
            camera_conflict: draw.history.conflict,
            previous_camera,
            previous: draw.history.previous,
        };
        let mut bytes = MaybeUninit::uninit();
        let (decision, reason) = match unsafe { decide_core(&input) } {
            Ok(decision) => {
                let packed = unsafe {
                    rsf_anomaly_motion_pack_core(
                        &decision.previous_wvp,
                        decision.bones.data,
                        decision.bones.count,
                        &decision.flags,
                        &mut bytes,
                    )
                };
                (
                    decision,
                    if packed {
                        HistoryReason::Valid
                    } else {
                        HistoryReason::TooManyBones
                    },
                )
            }
            Err(reason) => {
                rsf_anomaly_motion_pack_core_invalid(&mut bytes);
                (CoreDecision::default(), reason)
            }
        };
        let valid = reason == HistoryReason::Valid;
        if let Some(band) = draw.band {
            if reason == HistoryReason::NoPreviousInstance {
                self.counters.band_no_previous[band] += 1;
            }
            if valid {
                self.counters.band_valid[band] += 1;
            } else {
                self.counters.band_invalid[band] += 1;
            }
        }
        self.counters.reason(reason, draw.class);
        let mut uploaded = self.upload(draw.context, RSF_MOTION_CORE, unsafe {
            bytes.assume_init_ref()
        });
        if tessellated {
            let mut tess = MaybeUninit::uninit();
            let packed = rsf_anomaly_motion_pack_tess(
                &decision.previous_wv,
                &decision.previous_projection,
                valid,
                &mut tess,
            );
            self.counters.tess_pack_refused += u64::from(valid && !packed);
            uploaded = uploaded
                && self.upload(draw.context, RSF_MOTION_TESS, unsafe {
                    tess.assume_init_ref()
                });
        }
        uploaded
    }
}

#[derive(Default)]
struct History {
    previous: DrawSnapshotView,
    conflict: bool,
    stale: bool,
    sample: Option<CameraSample>,
}

struct Draw<'a> {
    facts: &'a DrawFacts,
    context: Raw,
    stock: ShaderObjects,
    state: DrawState,
    class: usize,
    band: Option<usize>,
    history: History,
}

impl Stage {
    fn arm_replay(&self, context: Raw) {
        if self.invalidate.load(Relaxed) {
            window::arm(window::Draw {
                context,
                action: window::Action::Replay,
            });
        }
    }

    fn poison_only(&self, render: &mut Render, context: Raw) {
        if !self.foliage.load(Relaxed) || context.is_null() {
            return;
        }
        let mut shaders = ShaderObjects::default();
        unsafe { (self.platform.bound_shaders)(context, &mut shaders, false) };
        let vertex = lock(&self.registry).get(shaders.vs);
        if let Some(vertex) = vertex.filter(|v| v.family == FOLIAGE_DETAIL && v.foliage.usable) {
            render.poison_batch(self.platform, &vertex.foliage);
        }
    }

    pub(super) fn flush(&self, facts: &DrawFacts) {
        self.cancel_draw();
        if !self.active.load(Acquire) {
            return;
        }
        let p = self.platform;
        let mut render = lock(&self.render);
        if !self.active.load(Acquire) {
            return;
        }
        let backend = unsafe { (p.backend)() };
        if !render.window.open {
            if render.binder.cache_may_hold_motion != 0 && !render.device_view().rtv.is_null() {
                render.bind_draw(p, backend, false);
            }
            if facts.detail {
                self.poison_only(&mut render, unsafe { (p.context)() });
            }
            return;
        }
        render.counters.window_draws += 1;
        let context = render.window.context;
        let wanted = render.window.target_ok && facts.main_pass();
        render.bind_draw(p, backend, wanted);
        let history = render.record_history(p, facts);
        if render.binder.bound == 0 || context.is_null() {
            if facts.detail {
                self.poison_only(&mut render, context);
            }
            return;
        }

        let mut stock = ShaderObjects::default();
        unsafe { (p.bound_shaders)(context, &mut stock, true) };
        let staged = [stock.hs, stock.ds, stock.gs]
            .iter()
            .any(|shader| !shader.is_null());
        let mut state = DrawState::default();
        unsafe { (p.draw_state)(context, &mut state) };
        let band = if facts.main_pass()
            && facts.in_visual
            && matches!(
                facts.kind,
                HARDWARE_SKINNED | SINGLE_BONE | SOFTWARE_SKINNED
            ) {
            let distance = world(p)
                .zip(history.sample.or_else(|| camera(p)))
                .map_or(f64::NAN, |(w, camera)| {
                    rsf_anomaly_motion_camera_distance(&camera.view, &engine_matrix(&w))
                });
            let band = rsf_anomaly_motion_distance_band(distance);
            if history.stale {
                render.counters.band_stale_palette[band] += 1;
            }
            Some(band)
        } else {
            None
        };
        let draw = Draw {
            facts,
            context,
            stock,
            state,
            class: facts.class(staged),
            band,
            history,
        };

        let (vertex, element, selection) = {
            let mut registry = lock(&self.registry);
            let vertex = registry.get(stock.vs);
            let name = registry.name(stock.vs as usize);
            let element = unsafe { rsf_anomaly_motion_model_element(name.as_ptr(), name.len()) };
            let selection = registry.select(
                stock,
                render.device_view().device,
                |vs, hs, ds, ps| unsafe { (p.link_shaders)(vs, hs, ds, ps) },
            );
            (vertex, element, selection)
        };
        if let Some(band) = band {
            match element {
                ModelElement::High => render.counters.band_hq[band] += 1,
                ModelElement::Low => render.counters.band_lq[band] += 1,
                ModelElement::Tessellated => render.counters.band_tess[band] += 1,
                ModelElement::Unknown => {}
            }
        }
        // Foliage staging is observed even when the PS has no verified variant.
        let mut foliage_bytes = MaybeUninit::uninit();
        let foliage_reason = if let Some(vertex) = vertex.filter(|v| foliage_family(v.family)) {
            render.foliage_bytes(self, &draw, &vertex, &mut foliage_bytes)
        } else {
            if facts.detail {
                self.poison_only(&mut render, context);
            }
            rsf_anomaly_motion_pack_foliage_invalid(&mut foliage_bytes);
            HistoryReason::Foliage
        };
        let chain = match selection {
            Ok(chain) => chain,
            Err(reason) => {
                render.counters.unpaired(&draw, reason);
                self.arm_replay(context);
                return;
            }
        };
        let tessellated = chain.tessellated();
        let family = chain.vertex.family;
        let foliage = foliage_family(family);
        let mut uploaded = if foliage {
            let valid = foliage_reason == HistoryReason::Valid;
            render.counters.reason(foliage_reason, draw.class);
            match (family == FOLIAGE_TREE, valid) {
                (true, true) => render.counters.tree_valid += 1,
                (true, false) => render.counters.tree_invalid += 1,
                (false, true) => render.counters.detail_valid += 1,
                (false, false) => render.counters.detail_invalid += 1,
            }
            render.upload(context, RSF_MOTION_FOLIAGE, unsafe {
                foliage_bytes.assume_init_ref()
            })
        } else {
            render.upload_core(&draw, &chain.vertex, tessellated)
        };
        let mut inputs = PixelInputs {
            viewport: draw.state.viewport,
            jitter: draw.state.jitter,
        };
        if foliage {
            // Foliage previous clip already removes the previous jitter.
            inputs.jitter[2..].fill(0.);
        }
        let mut bytes = MaybeUninit::uninit();
        rsf_anomaly_motion_pack_pixel(&inputs, &mut bytes);
        uploaded = uploaded
            && render.upload(context, RSF_MOTION_PIXEL, unsafe {
                bytes.assume_init_ref()
            });
        if !uploaded {
            if foliage {
                render.counters.stock_foliage(family);
            }
            render.counters.upload_failures += 1;
            self.arm_replay(context);
            return;
        }
        render.counters.motion_draws += 1;
        render.counters.tess_swapped += u64::from(tessellated);
        if family == FOLIAGE_TREE {
            render.counters.tree_replaced += 1;
        } else if family == FOLIAGE_DETAIL {
            render.counters.detail_replaced += 1;
        }
        unsafe { (p.set_shaders)(context, &chain.shaders(), tessellated) };
        window::arm(window::Draw {
            context,
            action: window::Action::Restore { stock, chain },
        });
    }
}

impl Counters {
    fn unpaired(&mut self, draw: &Draw<'_>, refusal: Refusal) {
        match refusal {
            Refusal::Missing => {}
            Refusal::Geometry => self.tess_geometry += 1,
            Refusal::TessMissing => self.tess_no_variant += 1,
            Refusal::TessUnlinked => self.tess_unlinked += 1,
        }
        if draw.facts.in_visual && draw.facts.kind == TREE {
            self.stock_foliage(FOLIAGE_TREE);
        }
        if draw.facts.detail {
            self.stock_foliage(FOLIAGE_DETAIL);
        }
        self.unpaired_draws += 1;
        self.class_unpaired[draw.class] += 1;
        if let Some(band) = draw.band {
            self.band_unpaired[band] += 1;
        }
        self.unpaired_pair(draw.stock, draw.class);
    }
}
