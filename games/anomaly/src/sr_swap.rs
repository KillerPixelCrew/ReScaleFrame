// SPDX-License-Identifier: MIT
//! Anomaly SR world-window swap and restore (docs/research/anomaly-sr-seam.md 1.4 and sections 9 to 11).
//!
//! Between the prologue of CRenderTarget::phase_scene_prepare (H1) and the S-B seam, the engine render targets point
//! at RSF-owned twins. Stage 2 also exchanges Device.dwWidth/dwHeight, HW.pBaseZB, HW.pBaseRT and the SSAO helper's
//! static viewport. Stage 3 jitters Device.mProject/mFullTransform and the first-person projections, and lowers the
//! sampler mip bias. Every exit path restores them.
//!
//! This owner decides eligibility, keys the twins and sequences each transaction. The native leaf performs every
//! guarded engine copy, the D3D11 work and the engine calls through `rsf_anomaly_swap_platform`. All calls arrive on
//! the engine render thread under the SR stage guard.

use crate::sr_swap_abi::*;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};

type Fields = rsf_anomaly_swap_fields;
type Target = rsf_anomaly_swap_target;
type Facts = rsf_anomaly_swap_facts;
type Spec = rsf_anomaly_swap_spec;
type Request = rsf_anomaly_swap_request;
type Twins = rsf_anomaly_swap_twins;
type Layout = rsf_anomaly_swap_layout;
type Platform = rsf_anomaly_swap_platform;
type View = rsf_anomaly_swap_view;
/// X-Ray Fmatrix bytes: row-major storage, row vectors (clip = v * M).
type Matrix = [f32; 16];

const SLOTS: usize = RSF_ANOMALY_SWAP_SLOT_COUNT as usize;
const STAGE1_SLOTS: usize = RSF_ANOMALY_SWAP_STAGE1_SLOT_COUNT as usize;
const GENERIC0: usize = 0;
const POSITION: usize = 2;
const GENERIC1: usize = 5;
const POINTER: u32 = size_of::<*mut c_void>() as u32;
const MAX_CACHE_SLOTS: u32 = 16;

const SKIP_NAMES: [&CStr; RSF_ANOMALY_SWAP_SKIP_COUNT as usize] =
    [c"off", c"scope", c"msaa", c"layout", c"twins", c"depth"];
const RESTORE_NAMES: [&CStr; RSF_ANOMALY_SWAP_RESTORE_COUNT as usize] = [
    c"seam",
    c"combine_end",
    c"stale_prepare",
    c"shutdown",
    c"rollback",
    c"frame_end",
];

/// An engine address or a pointer value read from engine memory. The native leaf validates every access.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Addr(*mut c_void);

impl Default for Addr {
    fn default() -> Self {
        Self(null_mut())
    }
}

impl Addr {
    fn is_null(self) -> bool {
        self.0.is_null()
    }

    fn at(self, offset: u32) -> Self {
        Self(self.0.cast::<u8>().wrapping_add(offset as usize).cast())
    }
}

/// Values the guarded copies may fill byte for byte: every bit pattern is valid.
trait Plain: Copy {}
impl Plain for u32 {}
impl Plain for i32 {}
impl Plain for f32 {}
impl Plain for Addr {}
impl Plain for *mut c_void {}
impl<T: Plain, const N: usize> Plain for [T; N] {}
impl Plain for u8 {}

fn same_fields(a: &Fields, b: &Fields) -> bool {
    a.surface == b.surface
        && a.rtv == b.rtv
        && a.dsv == b.dsv
        && a.uav == b.uav
        && a.width == b.width
        && a.height == b.height
        && a.format == b.format
        && a.texture_surface == b.texture_surface
        && a.desc_cache == b.desc_cache
        && a.desc == b.desc
        && a.srv == b.srv
}

/// The jittered copy of an unjittered engine projection, or None for a non-finite matrix or sample or an extent outside
/// 2..=16384. The sample is in render pixels, +X right, +Y down. For row vectors the NDC offset is
/// M[i][0] += 2x/W * M[i][3] and M[i][1] -= 2y/H * M[i][3]. The arithmetic runs in double precision like
/// sr_jitter.h, so untouched elements are returned bit-exact and both implementations agree bit for bit.
fn jitter_projection(m: &Matrix, x: f32, y: f32, width: u32, height: u32) -> Option<Matrix> {
    let valid = |extent: u32| (2..=16384).contains(&extent);
    if !valid(width)
        || !valid(height)
        || !x.is_finite()
        || !y.is_finite()
        || m.iter().any(|value| !value.is_finite())
    {
        return None;
    }
    let sx = 2.0 * f64::from(x) / f64::from(width);
    let sy = 2.0 * f64::from(y) / f64::from(height);
    let mut out = *m;
    for (row, source) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(m.as_chunks::<4>().0)
    {
        let w = f64::from(source[3]);
        let column0 = f64::from(source[0]) + sx * w;
        let column1 = f64::from(source[1]) - sy * w;
        if !column0.is_finite() || !column1.is_finite() {
            return None;
        }
        row[0] = column0 as f32;
        row[1] = column1 as f32;
    }
    out.iter().all(|value| value.is_finite()).then_some(out)
}

/// Fmatrix::mul(A, B) in the engine's summation order: m[i][j] = sum_k A[k][j] * B[i][k]. Device.mFullTransform is
/// mul(mProject, mView).
fn engine_mul(a: &Matrix, b: &Matrix) -> Matrix {
    std::array::from_fn(|n| {
        let (i, j) = (n / 4, n % 4);
        a[j] * b[i * 4]
            + a[4 + j] * b[i * 4 + 1]
            + a[8 + j] * b[i * 4 + 2]
            + a[12 + j] * b[i * 4 + 3]
    })
}

#[derive(Debug, PartialEq)]
enum FirstPerson {
    Jitter,
    AlreadyJittered,
    Foreign,
}

/// The six first-person sites rebuild Device.mProject with build_projection (_31 = _32 = 0), upload it and call
/// rmNear. A projection this owner already wrote is never jittered again, and a projection with a nonzero _31/_32
/// that it did not write is left alone.
fn first_person_action(current: &Matrix, main: &Matrix, last: Option<&Matrix>) -> FirstPerson {
    if current == main || last.is_some_and(|last| current == last) {
        FirstPerson::AlreadyJittered
    } else if current[8] == 0.0 && current[9] == 0.0 {
        FirstPerson::Jitter
    } else {
        FirstPerson::Foreign
    }
}

/// One axis of a twinned target: Full (the output extent) becomes the render extent, Half (output / 2, where allowed)
/// becomes render / 2 with the engine's integer division. Returns the extent and whether the Half rule applied.
fn twin_extent(engine: u32, output: u32, render: u32, half_allowed: bool) -> Option<(u32, bool)> {
    if engine == 0 || output == 0 || render == 0 || render > output {
        None
    } else if engine == output {
        Some((render, false))
    } else if half_allowed && engine == output / 2 && render / 2 != 0 {
        Some((render / 2, true))
    } else {
        None
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct TwinKey {
    stage: u32,
    render_target: Addr,
    crt: [Addr; SLOTS],
    surface: [Addr; SLOTS],
    engine_dsv: Addr,
    engine_rtv: Addr,
    device: Addr,
    generation: u64,
    width: u32,
    height: u32,
    render_width: u32,
    render_height: u32,
}

/// Engine globals Stage 2 exchanges, with the values saved at the exchange.
#[derive(Clone, Copy, Default)]
struct Globals {
    device_size: Addr,
    base_zb: Addr,
    base_rt: Addr,
    viewport: Addr,
    viewport_guard: Addr,
    width: u32,
    height: u32,
    zb: Addr,
    rt: Addr,
    viewport_swapped: bool,
    viewport_guard_was_set: bool,
    viewport_size: [f32; 2],
}

/// Stage 3: the projection the window changed, with the values to restore.
#[derive(Clone, Copy, Default)]
struct Projection {
    view: Addr,
    project: Addr,
    full: Addr,
    /// This window changed Device.mProject/mFullTransform.
    written: bool,
    /// RCache holds the jittered projection.
    uploaded: bool,
    /// The window's main projection carries the jitter.
    applied: bool,
    x: f32,
    y: f32,
    saved_project: Matrix,
    saved_full: Matrix,
    jittered: Matrix,
    last_first_person: Option<Matrix>,
    sun_suspended: bool,
    sun_full: Matrix,
}

#[derive(Clone, Copy, Default)]
struct Bias {
    field: Addr,
    applied: bool,
    saved: f32,
    window: f32,
}

#[derive(Default)]
struct RestoreState {
    targets: u16,
    globals: bool,
    caches: bool,
    reason: u32,
}

impl RestoreState {
    fn pending(&self) -> bool {
        self.targets != 0 || self.globals || self.caches
    }
}

struct Swap {
    view: View,
    layout: Layout,
    platform: Platform,
    enabled: bool,
    have_twins: bool,
    key: TwinKey,
    failed_key: Option<TwinKey>,
    backend: Addr,
    render_target: Addr,
    globals: Globals,
    projection: Projection,
    bias: Bias,
    restore_state: RestoreState,
    spatial_recovery: bool,
}

impl Swap {
    fn new(layout: Layout, platform: Platform) -> Self {
        Self {
            view: View {
                stage: 1,
                ..View::default()
            },
            layout,
            platform,
            enabled: false,
            have_twins: false,
            key: TwinKey::default(),
            failed_key: None,
            backend: Addr::default(),
            render_target: Addr::default(),
            globals: Globals::default(),
            projection: Projection::default(),
            bias: Bias::default(),
            restore_state: RestoreState::default(),
            spatial_recovery: false,
        }
    }

    // ---- native leaf calls ----------------------------------------------------------------------------------------
    fn read<T: Plain>(&self, address: Addr, value: &mut T) -> bool {
        let Some(read) = self.platform.read else {
            return false;
        };
        // The leaf copies size_of::<T>() bytes from engine memory under its access guard into a live value.
        unsafe {
            read(
                self.platform.user,
                address.0,
                (value as *mut T).cast(),
                size_of::<T>(),
            )
        }
    }

    fn write<T: Plain>(&self, address: Addr, value: &T) -> bool {
        let Some(write) = self.platform.write else {
            return false;
        };
        // The leaf copies size_of::<T>() bytes into engine memory under its access guard.
        unsafe {
            write(
                self.platform.user,
                address.0,
                (value as *const T).cast(),
                size_of::<T>(),
            )
        }
    }

    fn set_viewport(&mut self, width: u32, height: u32) {
        if let Some(set_viewport) = self.platform.set_viewport {
            // The leaf sets the immediate context's viewport on the render thread.
            unsafe { set_viewport(self.platform.user, width, height) };
        }
        self.view.counters.viewports += 1;
    }

    fn upload_projection(&self, matrix: Addr) -> bool {
        // R_xforms::set_P(&RCache.xforms, matrix) under the leaf's exception guard.
        self.platform
            .upload_projection
            .is_some_and(|upload| unsafe { upload(self.platform.user, matrix.0) })
    }

    fn set_mip_bias(&self, value: f32) -> bool {
        // dx10SamplerStateCache::SetMipLODBias under the leaf's exception guard.
        self.platform
            .set_mip_bias
            .is_some_and(|set| unsafe { set(self.platform.user, value) })
    }

    // ---- state helpers ------------------------------------------------------------------------------------------------
    fn slots(&self) -> usize {
        if self.view.stage == 1 {
            STAGE1_SLOTS
        } else {
            SLOTS
        }
    }

    fn refuse(&mut self, reason: &'static CStr) {
        if self.view.refusal.is_null() {
            self.view.refusal = reason.as_ptr();
        }
    }

    fn refused(&self) -> bool {
        !self.view.refusal.is_null()
    }

    fn skip(&mut self, reason: u32) -> bool {
        self.view.counters.skipped[reason as usize] += 1;
        false
    }

    fn foreign(&mut self, reason: &'static CStr) {
        self.view.counters.foreign_changes += 1;
        self.refuse(reason);
    }

    fn fault(&mut self, reason: &'static CStr) {
        self.view.counters.write_faults += 1;
        self.refuse(reason);
    }

    fn publish(&mut self) {
        self.view.jitter_applied = self.projection.applied;
        self.view.mip_bias_applied = self.bias.applied;
        self.view.window_mip_bias = self.bias.window;
        self.view.saved_mip_bias = self.bias.saved;
    }

    fn load(&self, target: &Target, out: &mut Fields) -> bool {
        let l = &self.layout;
        let crt = Addr(target.crt);
        let texture = Addr(target.texture);
        self.read(crt.at(l.crt_surface), &mut out.surface)
            && self.read(crt.at(l.crt_rtv), &mut out.rtv)
            && self.read(crt.at(l.crt_dsv), &mut out.dsv)
            && self.read(crt.at(l.crt_uav), &mut out.uav)
            && self.read(crt.at(l.crt_width), &mut out.width)
            && self.read(crt.at(l.crt_height), &mut out.height)
            && self.read(crt.at(l.crt_format), &mut out.format)
            && self.read(texture.at(l.texture_surface), &mut out.texture_surface)
            && self.read(texture.at(l.texture_desc_cache), &mut out.desc_cache)
            && self.read(texture.at(l.texture_desc), &mut out.desc)
            && self.read(texture.at(l.texture_srv), &mut out.srv)
    }

    fn store(&self, target: &Target, fields: &Fields) -> bool {
        let l = &self.layout;
        let crt = Addr(target.crt);
        let texture = Addr(target.texture);
        self.write(crt.at(l.crt_surface), &fields.surface)
            && self.write(crt.at(l.crt_rtv), &fields.rtv)
            && self.write(crt.at(l.crt_dsv), &fields.dsv)
            && self.write(crt.at(l.crt_uav), &fields.uav)
            && self.write(crt.at(l.crt_width), &fields.width)
            && self.write(crt.at(l.crt_height), &fields.height)
            && self.write(crt.at(l.crt_format), &fields.format)
            && self.write(texture.at(l.texture_surface), &fields.texture_surface)
            && self.write(texture.at(l.texture_desc_cache), &fields.desc_cache)
            && self.write(texture.at(l.texture_desc), &fields.desc)
            && self.write(texture.at(l.texture_srv), &fields.srv)
    }

    /// Reads every world target of the current stage and checks it against the verified layout: a colour CRT of the
    /// expected format whose CTexture binds that same surface, at a size the stage can twin.
    fn read_targets(&self, facts: &Facts) -> Option<Request> {
        let stage = self.view.stage;
        let mut request = Request {
            stage,
            render_width: if stage == 1 {
                facts.device_width
            } else {
                facts.render_width
            },
            render_height: if stage == 1 {
                facts.device_height
            } else {
                facts.render_height
            },
            output_width: facts.device_width,
            output_height: facts.device_height,
            ..Request::default()
        };
        let render_target = Addr(facts.render_target);
        for slot in 0..self.slots() {
            let world = self.layout.world[slot];
            let mut target = Target::default();
            if !self.read(render_target.at(world.offset), &mut target.crt) {
                return None;
            }
            if target.crt.is_null() {
                // An option that is off: the engine never renders into it.
                if stage == 2 && world.optional {
                    continue;
                }
                return None;
            }
            if !self.read(
                Addr(target.crt).at(self.layout.crt_texture),
                &mut target.texture,
            ) || target.texture.is_null()
            {
                return None;
            }
            let mut fields = Fields::default();
            if !self.load(&target, &mut fields) {
                return None;
            }
            let f = fields;
            if f.surface.is_null()
                || f.rtv.is_null()
                || !f.dsv.is_null()
                || f.srv.is_null()
                || f.texture_surface != f.surface
                || (!f.desc_cache.is_null() && f.desc_cache != f.surface)
            {
                return None;
            }
            let format = if world.d3d_format != 0 {
                f.format == world.d3d_format
            } else {
                f.format == self.layout.format_r16f || f.format == self.layout.format_r32f
            };
            if !format {
                return None;
            }
            request.spec[slot] = if stage == 1 {
                if f.width != facts.device_width || f.height != facts.device_height {
                    return None;
                }
                Spec {
                    present: true,
                    width: f.width,
                    height: f.height,
                    promote: true,
                }
            } else {
                let (width, half_x) = twin_extent(
                    f.width,
                    facts.device_width,
                    facts.render_width,
                    world.half_allowed,
                )?;
                let (height, half_y) = twin_extent(
                    f.height,
                    facts.device_height,
                    facts.render_height,
                    world.half_allowed,
                )?;
                if half_x != half_y {
                    return None;
                }
                Spec {
                    present: true,
                    width,
                    height,
                    promote: slot < STAGE1_SLOTS,
                }
            };
            target.fields = f;
            target.present = true;
            request.engine[slot] = target;
        }
        // Distinct records and surfaces: two slots sharing a CRT would be exchanged twice.
        let engine = &request.engine[..self.slots()];
        for (i, a) in engine.iter().enumerate() {
            for b in &engine[i + 1..] {
                if a.present
                    && b.present
                    && (a.crt == b.crt || a.fields.surface == b.fields.surface)
                {
                    return None;
                }
            }
        }
        if stage == 2 {
            for offset in self.layout.absent {
                let mut crt = Addr::default();
                if !self.read(render_target.at(offset), &mut crt) || !crt.is_null() {
                    return None;
                }
            }
        }
        Some(request)
    }

    fn twins_match(&self, request: &Request) -> bool {
        for slot in 0..self.slots() {
            let spec = request.spec[slot];
            let twin = &self.view.twins.targets[slot];
            let original = &request.engine[slot].fields;
            if !spec.present {
                if !same_fields(twin, &Fields::default()) {
                    return false;
                }
                continue;
            }
            let format = if spec.promote {
                self.layout.format_a16b16g16r16f
            } else {
                original.format
            };
            if twin.surface.is_null()
                || twin.rtv.is_null()
                || !twin.dsv.is_null()
                || twin.srv.is_null()
                || twin.texture_surface != twin.surface
                || twin.desc_cache != twin.surface
                || twin.format != format
                || twin.width != spec.width
                || twin.height != spec.height
                || twin.uav.is_null() != original.uav.is_null()
                || twin.surface == original.surface
            {
                return false;
            }
        }
        let t = &self.view.twins;
        !(request.stage == 2
            && (t.depth_dsv.is_null()
                || t.depth_srv.is_null()
                || t.depth_dsv == request.engine_dsv
                || t.depth_width != request.render_width
                || t.depth_height != request.render_height
                || t.base_rt.is_null()
                || t.base_rt == request.engine_rtv))
    }

    /// CBackend::set_Textures returns early on T == _T and then compares CTexture*, which a surface swap does not
    /// change. Clearing T and every per-stage CTexture* slot forces the next set_Textures to bind the new SRVs.
    /// Stage 2 changes Device.dwWidth/dwHeight, which the screen_res and pos_decompression_params binders read when
    /// set_Constants processes a table; clearing ctable makes the next set_Element re-run every binder.
    fn invalidate_caches(&mut self) -> bool {
        if self.backend.is_null() {
            return true;
        }
        let null = Addr::default();
        let zeros = [0u8; (MAX_CACHE_SLOTS * POINTER) as usize];
        let mut ok = self.write(self.backend.at(self.layout.backend_texture_list), &null);
        for range in self.layout.backend_textures {
            if range.count != 0 {
                let len = (range.count * POINTER) as usize;
                let write = self.platform.write.expect("validated platform");
                // Only the verified pointer array is cleared, leaving adjacent RCache fields alone.
                ok = unsafe {
                    write(
                        self.platform.user,
                        self.backend.at(range.offset).0,
                        zeros.as_ptr().cast(),
                        len,
                    )
                } && ok;
            }
        }
        if self.view.stage == 2 {
            ok = self.write(self.backend.at(self.layout.backend_ctable), &null) && ok;
        }
        if !ok {
            self.fault(c"RCache caches could not be invalidated.");
        }
        ok
    }

    fn release_twins(&mut self) {
        if !self.have_twins || self.view.open || self.restore_state.pending() {
            return;
        }
        if let Some(drop_twins) = self.platform.drop_twins {
            // The leaf releases its own views and textures; the engine fields no longer name them.
            unsafe { drop_twins(self.platform.user) };
        }
        self.have_twins = false;
        self.view.twins = Twins::default();
        self.key = TwinKey::default();
        self.view.counters.twin_drops += 1;
    }

    /// MSVC magic static: 0 never initialised, -1 initialisation in progress, anything else initialised.
    fn viewport_initialised(&self) -> bool {
        let mut guard = 0i32;
        !self.globals.viewport_guard.is_null()
            && self.read(self.globals.viewport_guard, &mut guard)
            && guard != 0
            && guard != -1
    }

    fn open_globals(&mut self) -> bool {
        let g = self.globals;
        let mut ok = self.write(g.device_size, &self.view.render_width)
            && self.write(g.device_size.at(4), &self.view.render_height)
            && self.write(g.base_zb, &Addr(self.view.twins.depth_dsv))
            && self.write(g.base_rt, &Addr(self.view.twins.base_rt));
        if ok && !g.viewport.is_null() && self.viewport_initialised() {
            let size = g.viewport.at(self.layout.ssao_viewport_width);
            let mut saved = [0f32; 2];
            if self.read(size, &mut saved) {
                self.globals.viewport_size = saved;
                // Before the write: a partial write is restored too.
                self.globals.viewport_swapped = true;
                let window = [
                    self.view.render_width as f32 * 0.5,
                    self.view.render_height as f32 * 0.5,
                ];
                ok = self.write(size, &window);
                if ok {
                    self.view.counters.ssao_viewport_swaps += 1;
                }
            } else {
                ok = false;
            }
        }
        ok
    }

    /// `checked`: compare the window's values before restoring (not on a rollback, which may have written part of
    /// them). `engine_ran`: the engine has drawn inside the window, so the values it derived from the window are
    /// replaced with output-size ones too.
    fn restore_globals(&mut self, checked: bool, engine_ran: bool) -> bool {
        let g = self.globals;
        let mut size = [0u32; 2];
        if checked
            && self.read(g.device_size, &mut size)
            && (size[0] != self.view.render_width || size[1] != self.view.render_height)
        {
            self.foreign(c"Device.dwWidth/dwHeight changed inside the world window.");
        }
        let mut zb = Addr::default();
        if checked && self.read(g.base_zb, &mut zb) && zb.0 != self.view.twins.depth_dsv {
            self.foreign(c"HW.pBaseZB changed inside the world window.");
        }
        let mut rt = Addr::default();
        if checked && self.read(g.base_rt, &mut rt) && rt.0 != self.view.twins.base_rt {
            self.foreign(c"HW.pBaseRT changed inside the world window.");
        }
        let mut ok = self.write_verified(g.device_size, &[g.width, g.height])
            & self.write_verified(g.base_zb, &g.zb)
            & self.write_verified(g.base_rt, &g.rt);
        let viewport = g.viewport.at(self.layout.ssao_viewport_width);
        if g.viewport_swapped {
            let restored = self.write_verified(viewport, &g.viewport_size);
            self.globals.viewport_swapped = !restored;
            ok &= restored;
        } else if engine_ran
            && !g.viewport.is_null()
            && !g.viewport_guard_was_set
            && self.viewport_initialised()
        {
            // The first SSAO call of the process happened inside the window and stored render/2: store what stock
            // would have stored.
            ok &= self.write_verified(viewport, &[g.width as f32 * 0.5, g.height as f32 * 0.5]);
        }
        // CRenderTarget::dwWidth/dwHeight follow the last u_setrt (a render-size twin): make rmNear/rmNormal and the
        // next viewport describe the output again.
        if engine_ran && !self.render_target.is_null() {
            ok &= self.write_verified(self.render_target.at(self.layout.target_width), &g.width)
                & self.write_verified(self.render_target.at(self.layout.target_height), &g.height);
        }
        if !ok {
            self.fault(c"An engine global could not be restored.");
        }
        ok
    }

    fn write_verified<T: Plain + PartialEq>(&self, address: Addr, value: &T) -> bool {
        let mut current = *value;
        self.write(address, value) && self.read(address, &mut current) && current == *value
    }

    fn open_temporal(&mut self, facts: &Facts) -> bool {
        if facts.jitter
            && !facts.device_view.is_null()
            && !facts.device_project.is_null()
            && !facts.device_full_transform.is_null()
        {
            let (view_at, project, full) = (
                Addr(facts.device_view),
                Addr(facts.device_project),
                Addr(facts.device_full_transform),
            );
            self.projection.view = view_at;
            self.projection.project = project;
            self.projection.full = full;
            let mut saved_project = Matrix::default();
            let mut view = Matrix::default();
            let mut saved_full = Matrix::default();
            if !self.read(project, &mut saved_project)
                || !self.read(view_at, &mut view)
                || !self.read(full, &mut saved_full)
            {
                return false;
            }
            self.projection.saved_project = saved_project;
            self.projection.saved_full = saved_full;
            match jitter_projection(
                &saved_project,
                facts.jitter_x,
                facts.jitter_y,
                self.view.render_width,
                self.view.render_height,
            ) {
                // A non-finite projection: this window renders unjittered.
                None => self.view.counters.jitter_invalid += 1,
                Some(jittered) => {
                    self.projection.jittered = jittered;
                    let jittered_full = engine_mul(&jittered, &view);
                    // Before the writes: a partial write is restored too.
                    self.projection.written = true;
                    if !self.write(project, &jittered) || !self.write(full, &jittered_full) {
                        return false;
                    }
                    // Before the call: a faulting upload is still followed by the restoring upload.
                    self.projection.uploaded = true;
                    if !self.upload_projection(project) {
                        return false;
                    }
                    self.projection.applied = true;
                    self.projection.x = facts.jitter_x;
                    self.projection.y = facts.jitter_y;
                    self.view.counters.jittered += 1;
                }
            }
        }
        if facts.mip_bias && !facts.sampler_bias.is_null() {
            let field = Addr(facts.sampler_bias);
            self.bias.field = field;
            let mut saved = 0f32;
            if !self.read(field, &mut saved) {
                return false;
            }
            self.bias.saved = saved;
            if !saved.is_finite() {
                return false;
            }
            let window = saved + facts.mip_bias_delta;
            self.bias.window = window;
            if window == saved {
                // SetMipLODBias would return early: nothing to restore.
                self.view.counters.mip_unchanged += 1;
            } else {
                self.bias.applied = true;
                let mut now = 0f32;
                if !self.set_mip_bias(window) || !self.read(field, &mut now) || now != window {
                    return false;
                }
                self.view.counters.mip_applied += 1;
            }
        }
        true
    }

    fn restore_temporal(&mut self, reason: u32) {
        let rollback = reason == RSF_ANOMALY_SWAP_RESTORE_ROLLBACK;
        let p = self.projection;
        if p.written {
            // Outside the temporary sun window, first-person sites restore the jittered main projection themselves.
            // Anything else is foreign: restore anyway and refuse further swaps.
            let mut current = Matrix::default();
            if !rollback
                && !p.sun_suspended
                && self.read(p.project, &mut current)
                && current != p.jittered
            {
                self.foreign(c"Device.mProject changed inside the world window.");
            }
            let mut ok =
                self.write(p.project, &p.saved_project) & self.write(p.full, &p.saved_full);
            // Shutdown writes engine memory only; the next frame's SetCacheXform uploads Device.mProject again.
            if ok && p.uploaded && reason != RSF_ANOMALY_SWAP_RESTORE_SHUTDOWN {
                ok = self.upload_projection(p.project);
            }
            if ok {
                self.view.counters.projection_restores += 1;
                self.projection.written = false;
                self.projection.uploaded = false;
            } else {
                self.fault(c"Device.mProject could not be restored.");
            }
            self.projection.sun_suspended = false;
        }
        let b = self.bias;
        if b.applied {
            let mut current = 0f32;
            if !rollback && self.read(b.field, &mut current) && current != b.window {
                self.foreign(c"The sampler mip bias changed inside the world window.");
            }
            // Also on shutdown: nothing else would ever put the user's bias back.
            if self.set_mip_bias(b.saved) {
                self.view.counters.mip_restored += 1;
            } else {
                self.fault(c"The sampler mip bias could not be restored.");
            }
            self.bias.applied = false;
        }
    }

    fn restore(&mut self, reason: u32) -> bool {
        let closing = std::mem::take(&mut self.view.open);
        if closing {
            self.restore_state.reason = reason;
            self.restore_state.caches = true;
            self.restore_temporal(reason);
        } else if !self.restore_state.pending() {
            return true;
        }
        let checked = closing && reason != RSF_ANOMALY_SWAP_RESTORE_ROLLBACK;
        for slot in 0..self.slots() {
            let target = self.view.engine[slot];
            let bit = 1 << slot;
            if self.restore_state.targets & bit == 0 {
                continue;
            }
            if !closing {
                // A later cleanup retry must still see the original binding
                // chain before it writes through saved engine addresses.
                let mut crt = Addr::default();
                let mut texture = Addr::default();
                if !self.read(
                    self.render_target.at(self.layout.world[slot].offset),
                    &mut crt,
                ) || crt.0 != target.crt
                    || !self.read(crt.at(self.layout.crt_texture), &mut texture)
                    || texture.0 != target.texture
                {
                    self.fault(c"The engine target changed before restoration could be verified.");
                    continue;
                }
            }
            // Inside the window nothing but the engine's own binds should touch these fields. A different value means
            // something replaced them (surface_set on a twinned texture): restore anyway and refuse further swaps.
            let mut current = Fields::default();
            if checked
                && self.load(&target, &mut current)
                && !same_fields(&current, &self.view.twins.targets[slot])
            {
                self.foreign(c"A twinned engine field changed inside the world window.");
            }
            if self.store(&target, &target.fields)
                && self.load(&target, &mut current)
                && same_fields(&current, &target.fields)
            {
                self.restore_state.targets &= !bit;
            } else {
                self.fault(c"An engine field write failed while restoring.");
            }
        }
        if self.restore_state.globals {
            let engine_ran = self.restore_state.reason != RSF_ANOMALY_SWAP_RESTORE_ROLLBACK;
            self.restore_state.globals = !self.restore_globals(checked, engine_ran);
        }
        // If an engine reference is still live, invalidate again after the retry:
        // the engine may have cached the same twin in the meantime.
        self.restore_state.caches = !self.invalidate_caches();
        if self.restore_state.pending() {
            return false;
        }
        self.view.counters.restores[self.restore_state.reason as usize] += 1;
        true
    }

    /// Restores after the engine has drawn into the window and puts the viewport back to output size, like every
    /// other exit that closes a window mid-frame.
    fn close_with_output(&mut self, reason: u32) {
        self.restore(reason);
        if self.view.stage == 2 {
            self.set_viewport(self.view.output_width, self.view.output_height);
        }
    }

    // ---- entry points -------------------------------------------------------------------------------------------------
    fn close_stale(&mut self) {
        if self.view.open {
            self.close_with_output(RSF_ANOMALY_SWAP_RESTORE_STALE_PREPARE);
        }
    }

    fn prepare(&mut self, facts: &Facts) -> bool {
        self.view.counters.prepares += 1;
        self.close_stale();
        // After close_stale, which needs the previous window's values.
        self.projection = Projection::default();
        self.bias = Bias::default();
        self.spatial_recovery = false;
        if !self.enabled || self.refused() {
            return self.skip(RSF_ANOMALY_SWAP_SKIP_OFF);
        }
        if facts.scope {
            return self.skip(RSF_ANOMALY_SWAP_SKIP_SCOPE);
        }
        if facts.msaa {
            return self.skip(RSF_ANOMALY_SWAP_SKIP_MSAA);
        }
        self.view.stage = if facts.stage == 2 { 2 } else { 1 };
        let stage2 = self.view.stage == 2;
        if stage2
            && (facts.device_size.is_null()
                || facts.base_zb.is_null()
                || facts.base_rt.is_null()
                || facts.render_width == 0
                || facts.render_height == 0
                || facts.render_width > facts.device_width
                || facts.render_height > facts.device_height)
        {
            self.refuse(c"The Stage 2 frame facts are incomplete.");
            return self.skip(RSF_ANOMALY_SWAP_SKIP_LAYOUT);
        }
        let read = if facts.render_target.is_null() {
            None
        } else {
            self.read_targets(facts)
        };
        let Some(mut request) = read else {
            self.refuse(if stage2 {
                c"A world target does not match the verified CRT/CTexture layout."
            } else {
                c"generic0/generic_temp do not match the verified CRT/CTexture layout."
            });
            return self.skip(RSF_ANOMALY_SWAP_SKIP_LAYOUT);
        };
        if stage2 {
            let mut size = [0u32; 2];
            if !self.read(Addr(facts.device_size), &mut size)
                || size != [facts.device_width, facts.device_height]
            {
                self.refuse(c"Device.dwWidth/dwHeight do not match the frame facts.");
                return self.skip(RSF_ANOMALY_SWAP_SKIP_LAYOUT);
            }
            if !self.read(Addr(facts.base_zb), &mut request.engine_dsv)
                || request.engine_dsv.is_null()
            {
                self.refuse(c"HW.pBaseZB is unreadable or null.");
                return self.skip(RSF_ANOMALY_SWAP_SKIP_DEPTH);
            }
            if !self.read(Addr(facts.base_rt), &mut request.engine_rtv)
                || request.engine_rtv.is_null()
            {
                self.refuse(c"HW.pBaseRT is unreadable or null.");
                return self.skip(RSF_ANOMALY_SWAP_SKIP_LAYOUT);
            }
        }
        let key = TwinKey {
            stage: self.view.stage,
            render_target: Addr(facts.render_target),
            crt: std::array::from_fn(|slot| Addr(request.engine[slot].crt)),
            surface: std::array::from_fn(|slot| Addr(request.engine[slot].fields.surface)),
            engine_dsv: Addr(request.engine_dsv),
            engine_rtv: Addr(request.engine_rtv),
            device: Addr(facts.device),
            generation: facts.generation,
            width: facts.device_width,
            height: facts.device_height,
            render_width: request.render_width,
            render_height: request.render_height,
        };
        // Allocation failed for this exact shape.
        if self.failed_key == Some(key) {
            return self.skip(RSF_ANOMALY_SWAP_SKIP_TWINS);
        }
        if !self.have_twins || key != self.key {
            // Reset, resolution or scale change, or a recreated CRenderTarget: drop the old twins, build new ones.
            self.release_twins();
            let mut made = Twins::default();
            let result = self
                .platform
                .make_twins
                .map_or(RSF_ANOMALY_SWAP_MAKE_FAILED, |make| {
                    // The leaf creates twins for `request` and fills `made`; neither outlives the call.
                    unsafe { make(self.platform.user, &request, &mut made) }
                });
            match result {
                RSF_ANOMALY_SWAP_MAKE_CREATED => {}
                RSF_ANOMALY_SWAP_MAKE_FAILED => {
                    // Not a refusal: this shape stays stock, a new shape is tried again.
                    self.view.counters.allocation_failures += 1;
                    self.failed_key = Some(key);
                    return self.skip(RSF_ANOMALY_SWAP_SKIP_TWINS);
                }
                RSF_ANOMALY_SWAP_MAKE_DEPTH => {
                    self.refuse(
                        c"HW.pBaseZB is not a single-sample D24S8 depth view of output size.",
                    );
                    return self.skip(RSF_ANOMALY_SWAP_SKIP_DEPTH);
                }
                _ => {
                    self.refuse(c"An engine target cannot be twinned.");
                    return self.skip(RSF_ANOMALY_SWAP_SKIP_TWINS);
                }
            }
            self.view.twins = made;
            self.have_twins = true;
            if !self.twins_match(&request) {
                self.release_twins();
                self.refuse(c"Created twins do not match the engine targets.");
                return self.skip(RSF_ANOMALY_SWAP_SKIP_TWINS);
            }
            self.key = key;
            self.failed_key = None;
            self.view.counters.twin_generations += 1;
        }
        self.view.counters.eligible += 1;
        self.backend = Addr(facts.backend);
        self.render_target = Addr(facts.render_target);
        self.view.engine = request.engine;
        self.view.output_width = facts.device_width;
        self.view.output_height = facts.device_height;
        self.view.render_width = request.render_width;
        self.view.render_height = request.render_height;
        self.globals = Globals::default();
        if stage2 {
            self.globals = Globals {
                device_size: Addr(facts.device_size),
                base_zb: Addr(facts.base_zb),
                base_rt: Addr(facts.base_rt),
                viewport: Addr(facts.ssao_viewport),
                viewport_guard: Addr(facts.ssao_viewport_guard),
                width: facts.device_width,
                height: facts.device_height,
                zb: Addr(request.engine_dsv),
                rt: Addr(request.engine_rtv),
                ..Globals::default()
            };
            self.globals.viewport_guard_was_set = self.viewport_initialised();
        }
        self.view.open = true;
        let mut written = true;
        for slot in 0..self.slots() {
            if !written {
                break;
            }
            let target = self.view.engine[slot];
            if target.present {
                self.restore_state.targets |= 1 << slot;
                written = self.store(&target, &self.view.twins.targets[slot]);
            }
        }
        if written && stage2 {
            self.restore_state.globals = true;
            written = self.open_globals();
        }
        if !written {
            self.fault(c"An engine field write failed during the twin exchange.");
            self.restore(RSF_ANOMALY_SWAP_RESTORE_ROLLBACK);
            return false;
        }
        if !self.invalidate_caches() {
            self.restore(RSF_ANOMALY_SWAP_RESTORE_ROLLBACK);
            return false;
        }
        if stage2 && !self.open_temporal(facts) {
            self.fault(c"The Stage 3 projection or mip bias could not be applied.");
            self.restore(RSF_ANOMALY_SWAP_RESTORE_ROLLBACK);
            return false;
        }
        self.view.counters.swapped += 1;
        true
    }

    /// phase_scene_prepare bound rt_Position (render size) with u_setrt(Device.dwWidth, ...), but the viewport is
    /// still the one CRender::Render's rmNormal set from the previous frame's output-size target.
    fn prepared(&mut self) {
        if self.view.open && self.view.stage == 2 {
            self.set_viewport(self.view.render_width, self.view.render_height);
        }
    }

    fn seam(&mut self) -> bool {
        if !self.view.open {
            self.view.counters.seams_without_window += 1;
            return false;
        }
        self.view.counters.seams += 1;
        if !self.restore(RSF_ANOMALY_SWAP_RESTORE_SEAM) {
            if self.view.stage == 2 {
                self.set_viewport(self.view.output_width, self.view.output_height);
            }
            return false;
        }
        self.view.counters.round_trips += 1;
        let ok = if self.view.stage == 1 {
            let twin = &raw const self.view.twins.targets[GENERIC0];
            let engine = &raw const self.view.engine[GENERIC0].fields;
            // The leaf reads both records during the call; this owner does not change them meanwhile.
            self.platform
                .round_trip
                .is_some_and(|run| unsafe { run(self.platform.user, twin, engine) })
        } else {
            let inputs = rsf_anomaly_swap_upscale {
                generic0_twin: &raw const self.view.twins.targets[GENERIC0],
                position_twin: &raw const self.view.twins.targets[POSITION],
                generic1_twin: &raw const self.view.twins.targets[GENERIC1],
                depth_srv: self.view.twins.depth_srv,
                generic0: &raw const self.view.engine[GENERIC0].fields,
                position: &raw const self.view.engine[POSITION].fields,
                generic1: &raw const self.view.engine[GENERIC1].fields,
                engine_dsv: self.globals.zb.0,
                render_width: self.view.render_width,
                render_height: self.view.render_height,
                output_width: self.view.output_width,
                output_height: self.view.output_height,
                spatial_recovery: self.spatial_recovery,
            };
            // The leaf runs the seam pass on the restored engine targets during the call.
            let ok = self
                .platform
                .upscale
                .is_some_and(|run| unsafe { run(self.platform.user, &inputs) });
            self.set_viewport(self.view.output_width, self.view.output_height);
            ok
        };
        if !ok {
            self.view.counters.round_trip_failures += 1;
            self.refuse(
                c"The seam pass failed; engine generic0 holds no world image for this frame.",
            );
        }
        true
    }

    fn combine_end(&mut self) {
        self.view.counters.combine_ends += 1;
        if !self.view.open {
            return;
        }
        self.close_with_output(RSF_ANOMALY_SWAP_RESTORE_COMBINE_END);
        self.refuse(
            c"phase_combine returned without the S-B seam; the frame was rendered without its world image.",
        );
    }

    fn frame_end(&mut self) {
        if !self.view.open {
            return;
        }
        self.close_with_output(RSF_ANOMALY_SWAP_RESTORE_FRAME_END);
        self.refuse(c"A world window was still open at the end of the frame.");
    }

    /// Shadow fitting temporarily borrows an unjittered Device camera, but its accumulation still samples the
    /// jittered G-buffer. The saved raster jitter, not sun_suspended or the current Device.mProject, owns these rays.
    fn position_constant(&mut self, constant: *mut c_void) {
        if !self.view.open || !self.projection.applied {
            self.view.counters.position_stock += 1;
            return;
        }
        let (x, y) = (self.projection.x, self.projection.y);
        // The leaf corrects the engine's constant buffer under its exception guard.
        let corrected = self
            .platform
            .correct_position_constant
            .is_some_and(|correct| unsafe { correct(self.platform.user, constant, x, y) });
        if corrected {
            self.view.counters.position_corrected += 1;
        } else {
            self.view.counters.position_refused += 1;
            self.refuse(c"Deferred position constant correction refused.");
        }
    }

    /// Fit shadows to the saved main camera. The token is true only when this call suspended the jitter.
    fn sun_begin(&mut self) -> bool {
        self.view.counters.sun_calls += 1;
        let p = self.projection;
        // A reentrant call borrows the already unjittered camera and must not restore the outer call's token.
        if !self.view.open || self.spatial_recovery || !p.applied || !p.written || p.sun_suspended {
            return false;
        }
        let mut current = Matrix::default();
        let mut sun_full = Matrix::default();
        let mut ok = self.read(p.project, &mut current)
            && current == p.jittered
            && self.read(p.full, &mut sun_full);
        if ok {
            self.projection.sun_full = sun_full;
            // Before the writes: the rollback restores partial writes too.
            self.projection.sun_suspended = true;
            let project = self.write(p.project, &p.saved_project);
            let full = self.write(p.full, &p.saved_full);
            ok = project && full;
        }
        if ok {
            self.view.counters.sun_unjittered += 1;
            return true;
        }
        self.view.counters.sun_refused += 1;
        self.fault(c"The sun cascade camera could not be unjittered.");
        self.recover_sun_camera();
        false
    }

    /// Restore the exact jittered pair and upload it through set_P: the original leaves RCache with its unjittered
    /// Device projection, and writing Device alone would leave the following world passes with wrong constants.
    fn sun_end(&mut self, suspended: bool) {
        if !suspended || !self.projection.sun_suspended {
            return;
        }
        self.projection.sun_suspended = false;
        let p = self.projection;
        let project = self.write(p.project, &p.jittered);
        let full = self.write(p.full, &p.sun_full);
        if project && full && self.upload_projection(p.project) {
            self.view.counters.sun_restored += 1;
            return;
        }
        self.view.counters.sun_refused += 1;
        self.fault(c"The jittered camera could not be restored after sun cascades.");
        self.recover_sun_camera();
    }

    fn recover_sun_camera(&mut self) {
        // World geometry already lives in the twins. Keep all target bindings and
        // the reduced viewport until the seam can transfer colour, position and depth.
        self.spatial_recovery = true;
        self.restore_temporal(RSF_ANOMALY_SWAP_RESTORE_ROLLBACK);
    }

    /// CRender::rmNear, after the original. Inside a jittered window, the site built Device.mProject (psHUD_FOV),
    /// mFullTransform = mProject * mView, and uploaded it; it restores the jittered main projection itself afterwards.
    /// The same pixel offset goes onto the fresh matrix once.
    fn rm_near(&mut self) {
        if !self.view.open {
            return;
        }
        self.view.counters.rm_near_in_window += 1;
        let p = self.projection;
        if self.spatial_recovery || !p.written || !p.applied || p.sun_suspended {
            return;
        }
        let mut current = Matrix::default();
        let mut view = Matrix::default();
        let mut saved_full = Matrix::default();
        if !self.read(p.project, &mut current)
            || !self.read(p.view, &mut view)
            || !self.read(p.full, &mut saved_full)
        {
            self.view.counters.fp_foreign += 1;
            return;
        }
        match first_person_action(&current, &p.jittered, p.last_first_person.as_ref()) {
            FirstPerson::AlreadyJittered => {
                self.view.counters.fp_already += 1;
                return;
            }
            FirstPerson::Foreign => {
                self.view.counters.fp_foreign += 1;
                return;
            }
            FirstPerson::Jitter => {}
        }
        let Some(jittered) = jitter_projection(
            &current,
            p.x,
            p.y,
            self.view.render_width,
            self.view.render_height,
        ) else {
            self.view.counters.jitter_invalid += 1;
            return;
        };
        let full = engine_mul(&jittered, &view);
        if !self.write(p.project, &jittered)
            || !self.write(p.full, &full)
            || !self.upload_projection(p.project)
        {
            let project = self.write(p.project, &current);
            let full = self.write(p.full, &saved_full);
            let uploaded = project && self.upload_projection(p.project);
            self.fault(c"A first-person projection could not be jittered.");
            if !full || !uploaded {
                self.fault(c"The first-person projection could not be restored.");
            }
            return;
        }
        self.projection.last_first_person = Some(jittered);
        self.view.counters.fp_jittered += 1;
    }

    fn shutdown(&mut self) -> bool {
        if !self.restore(RSF_ANOMALY_SWAP_RESTORE_SHUTDOWN) {
            return false;
        }
        self.release_twins();
        self.failed_key = None;
        true
    }
}

fn layout_valid(layout: &Layout) -> bool {
    layout.struct_size as usize >= size_of::<Layout>()
        && layout
            .backend_textures
            .iter()
            .all(|range| range.count <= MAX_CACHE_SLOTS)
}

fn platform_valid(platform: &Platform) -> bool {
    platform.read.is_some()
        && platform.write.is_some()
        && platform.make_twins.is_some()
        && platform.drop_twins.is_some()
        && platform.round_trip.is_some()
        && platform.upscale.is_some()
        && platform.set_viewport.is_some()
        && platform.upload_projection.is_some()
        && platform.set_mip_bias.is_some()
        && platform.correct_position_constant.is_some()
}

struct Handle {
    swap: Swap,
    published: View,
}

fn with<R>(handle: *mut Handle, fallback: R, call: impl FnOnce(&mut Swap) -> R) -> R {
    if handle.is_null() {
        return fallback;
    }
    // Only borrow the mutable owner. The native seam may read the disjoint
    // published view while this call is executing.
    let swap = unsafe { &mut (*handle).swap };
    let result = call(swap);
    swap.publish();
    unsafe { (&raw mut (*handle).published).write(swap.view) };
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_swap_create(
    layout: Option<&Layout>,
    platform: Option<&Platform>,
) -> *mut Handle {
    let (Some(layout), Some(platform)) = (layout, platform) else {
        return null_mut();
    };
    if !layout_valid(layout) || !platform_valid(platform) {
        return null_mut();
    }
    let swap = Swap::new(*layout, *platform);
    let published = swap.view;
    Box::into_raw(Box::new(Handle { swap, published }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_swap_destroy(swap: *mut Handle) {
    if !swap.is_null() {
        // The facade destroys its handle once, after shutdown restored the engine.
        drop(unsafe { Box::from_raw(swap) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_swap_view_of(swap: *const Handle) -> *const View {
    if swap.is_null() {
        null()
    } else {
        unsafe { &raw const (*swap).published }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_set_enabled(swap: *mut Handle, enabled: bool) {
    with(swap, (), |swap| swap.enabled = enabled);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_close_stale(swap: *mut Handle) {
    with(swap, (), Swap::close_stale);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_prepare(swap: *mut Handle, facts: Option<&Facts>) -> bool {
    let Some(facts) = facts else {
        return false;
    };
    with(swap, false, |swap| swap.prepare(facts))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_prepared(swap: *mut Handle) {
    with(swap, (), Swap::prepared);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_seam(swap: *mut Handle) -> bool {
    with(swap, false, Swap::seam)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_combine_end(swap: *mut Handle) {
    with(swap, (), Swap::combine_end);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_frame_end(swap: *mut Handle) {
    with(swap, (), Swap::frame_end);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_rm_near(swap: *mut Handle) {
    with(swap, (), Swap::rm_near);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_sun_begin(swap: *mut Handle) -> bool {
    with(swap, false, Swap::sun_begin)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_sun_end(swap: *mut Handle, suspended: bool) {
    with(swap, (), |swap| swap.sun_end(suspended));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_position_constant(swap: *mut Handle, constant: *mut c_void) {
    with(swap, (), |swap| swap.position_constant(constant));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_shutdown(swap: *mut Handle) -> bool {
    with(swap, true, Swap::shutdown)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_jitter_projection(
    projection: Option<&Matrix>,
    x: f32,
    y: f32,
    width: u32,
    height: u32,
    output: Option<&mut Matrix>,
) -> bool {
    let (Some(projection), Some(output)) = (projection, output) else {
        return false;
    };
    let Some(jittered) = jitter_projection(projection, x, y, width, height) else {
        return false;
    };
    *output = jittered;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_skip_name(skip: u32) -> *const c_char {
    SKIP_NAMES
        .get(skip as usize)
        .map_or(c"unknown", |name| name)
        .as_ptr()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_swap_restore_name(restore: u32) -> *const c_char {
    RESTORE_NAMES
        .get(restore as usize)
        .map_or(c"unknown", |name| name)
        .as_ptr()
}

#[cfg(test)]
mod tests;
