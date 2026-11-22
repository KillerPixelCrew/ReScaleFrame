// SPDX-License-Identifier: MIT
//! Anomaly draw-time history and the verified b13 register layouts.
//!
//! The motion stage consumes guarded draw facts and borrows previous draw data
//! through upload. The private C ABI also serves the native contract fixtures.

mod foliage;
#[cfg(windows)]
mod stage;
#[cfg(any(windows, test))]
mod window;

use crate::geometry_registry_abi::rsf_anomaly_geometry_key as InstanceKey;
use crate::shader_family::{CORE_MODEL, CORE_STATIC, FOLIAGE_DETAIL, FOLIAGE_TREE};
#[cfg(windows)]
pub(crate) use stage::record_retirement;
use std::borrow::Cow;
use std::collections::{BTreeMap, btree_map::Entry};
use std::ffi::c_char;
use std::mem::MaybeUninit;
use std::{ptr, slice};

#[derive(Clone, Copy, Default, PartialEq)]
#[repr(transparent)]
struct Mat4([f64; 16]);

impl Mat4 {
    const IDENTITY: Self = Self([
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]);

    fn usable(&self) -> bool {
        self.0.iter().all(|v| v.abs() <= 3.0e38)
    }

    fn compose(&self, rhs: &Self) -> Self {
        let mut matrix = Self::default();
        for (r, row) in matrix.0.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                *cell = (0..4).fold(0., |sum, k| sum + self.0[r * 4 + k] * rhs.0[k * 4 + c]);
            }
        }
        matrix
    }

    fn remove_jitter(&self, u: f32, v: f32) -> Self {
        let mut matrix = *self;
        for c in 0..4 {
            matrix.0[c] -= 2. * f64::from(u) * self.0[12 + c];
            matrix.0[4 + c] += 2. * f64::from(v) * self.0[12 + c];
        }
        matrix
    }
}

#[derive(Clone, Copy, PartialEq)]
#[repr(C)]
struct CameraSample {
    view: Mat4,
    projection: Mat4,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum CameraStatus {
    Recorded,
    Same,
    Conflict,
    NoFrame,
}

impl InstanceKey {
    fn valid(self) -> bool {
        self.owner != 0 && self.visual != 0 && self.generation != 0
    }

    fn identity(self) -> (u64, u64) {
        (self.owner, self.visual)
    }
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct PaletteView {
    data: *const Mat4,
    count: usize,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct DrawSnapshotView {
    world: *const Mat4,
    bones: PaletteView,
}

#[repr(C)]
struct SnapshotInput {
    draw: DrawSnapshotView,
    palette_layout: u64,
    deformation_constants: *const [f64; 4],
    deformation_count: usize,
    deformation_layout: u64,
    detail: *const f64,
    domain: u8,
    kind: u8,
    has_deformation: bool,
}

// The guarded draw capture owns the matrices through the synchronous flush.
#[cfg(windows)]
#[repr(C)]
struct GeometrySnapshotFacts {
    key: InstanceKey,
    world: *const Mat4,
    bones: *const Mat4,
    bone_count: usize,
    palette_layout: u64,
    view: u8,
    domain: u8,
    draw_kind: u8,
}

#[cfg(windows)]
const _: () = {
    assert!(size_of::<GeometrySnapshotFacts>() == 64);
    assert!(std::mem::offset_of!(GeometrySnapshotFacts, world) == 24);
    assert!(std::mem::offset_of!(GeometrySnapshotFacts, view) == 56);
    assert!(std::mem::offset_of!(GeometrySnapshotFacts, draw_kind) == 58);
};

#[cfg(windows)]
impl GeometrySnapshotFacts {
    fn kind(&self) -> Result<u8, u8> {
        use crate::native_geometry_abi::{HARDWARE_SKINNED, RIGID, SINGLE_BONE, UNSET};

        if self.view == UNSET || self.domain == UNSET {
            return Err(1); // MissingContext
        }
        match self.draw_kind {
            RIGID | HARDWARE_SKINNED | SINGLE_BONE => {}
            _ => return Err(4), // UnsupportedKind
        }
        if self.world.is_null() {
            return Err(3); // MissingWorld
        }
        if self.draw_kind == HARDWARE_SKINNED {
            if self.bones.is_null() || self.bone_count == 0 || self.palette_layout == 0 {
                return Err(5); // MissingPalette
            }
            return Ok(1); // InstanceKind::Skinned
        }
        // The engine already folded the single bone into the world matrix at the draw flush.
        Ok(0) // InstanceKind::Rigid
    }

    fn input(&self) -> Option<SnapshotInput> {
        let kind = self.kind().ok()?;
        let (bones, palette_layout) = if kind == 1 {
            (
                PaletteView {
                    data: self.bones,
                    count: self.bone_count,
                },
                self.palette_layout,
            )
        } else {
            (PaletteView::default(), 0)
        };
        Some(SnapshotInput {
            draw: DrawSnapshotView {
                world: self.world,
                bones,
            },
            palette_layout,
            deformation_constants: ptr::null(),
            deformation_count: 0,
            deformation_layout: 0,
            detail: ptr::null(),
            domain: self.domain,
            kind,
            has_deformation: false,
        })
    }
}

// The native geometry contract uses the same classifier for its owning fixture snapshots.
#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_snapshot_kind(
    capture: &GeometrySnapshotFacts,
    kind: &mut u8,
) -> u8 {
    match capture.kind() {
        Ok(value) => {
            *kind = value;
            7 // Captured
        }
        Err(refusal) => refusal,
    }
}

#[cfg(all(test, windows))]
#[test]
fn geometry_palette_is_borrowed_until_history_accepts_it() {
    use crate::native_geometry_abi::{HARDWARE_SKINNED, MAIN, SINGLE_BONE, WORLD};

    let world = Mat4::IDENTITY;
    let palette = [Mat4::IDENTITY; 2];
    let mut capture = GeometrySnapshotFacts {
        key: InstanceKey {
            owner: 1,
            visual: 2,
            generation: 3,
        },
        world: &world,
        bones: palette.as_ptr(),
        bone_count: palette.len(),
        palette_layout: 1,
        view: MAIN,
        domain: WORLD,
        draw_kind: HARDWARE_SKINNED,
    };
    let input = capture.input().unwrap();
    assert_eq!(input.draw.bones.data, palette.as_ptr());
    assert_eq!(input.draw.bones.count, palette.len());
    let snapshot = unsafe { Snapshot::borrow(&input) };
    assert!(matches!(snapshot.bones, Cow::Borrowed(_)));

    let mut history = MotionHistoryState::default();
    history.begin(
        0,
        FrameIdentity {
            source: 1,
            generation: 1,
            width: 1280,
            height: 720,
        },
    );
    assert!(
        history
            .record(0, capture.key, capture.domain, snapshot)
            .is_none()
    );
    let frame = history.views[0].current.as_ref().unwrap();
    let stored = frame.instances[&(capture.key, capture.domain)]
        .as_ref()
        .unwrap();
    assert!(matches!(stored.bones, Cow::Owned(_)));
    assert_ne!(stored.bones.as_ptr(), palette.as_ptr());

    capture.draw_kind = SINGLE_BONE;
    let input = capture.input().unwrap();
    assert_eq!(input.palette_layout, 0);
    assert_eq!(input.draw.bones.count, 0);
}

#[derive(PartialEq)]
struct Snapshot<'a> {
    world: Mat4,
    kind: u8,
    palette_layout: u64,
    bones: Cow<'a, [Mat4]>,
    deformation: Option<(u64, Cow<'a, [[f64; 4]]>)>,
    detail: Option<f64>,
}

// Empty native spans may carry null. Nonempty spans in this private ABI are
// guaranteed by the native caller to be aligned, initialized and live.
unsafe fn borrow_span<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(data, count) }
    }
}

impl Snapshot<'_> {
    unsafe fn borrow(input: &SnapshotInput) -> Snapshot<'_> {
        Snapshot {
            world: unsafe { *input.draw.world },
            kind: input.kind,
            palette_layout: input.palette_layout,
            bones: Cow::Borrowed(unsafe {
                borrow_span(input.draw.bones.data, input.draw.bones.count)
            }),
            deformation: input.has_deformation.then(|| {
                let values =
                    unsafe { borrow_span(input.deformation_constants, input.deformation_count) };
                (input.deformation_layout, Cow::Borrowed(values))
            }),
            detail: unsafe { input.detail.as_ref().copied() },
        }
    }

    fn into_owned(self) -> Snapshot<'static> {
        Snapshot {
            world: self.world,
            kind: self.kind,
            palette_layout: self.palette_layout,
            bones: Cow::Owned(self.bones.into_owned()),
            deformation: self
                .deformation
                .map(|(layout, values)| (layout, Cow::Owned(values.into_owned()))),
            detail: self.detail,
        }
    }

    fn compatible(&self, current: &Self) -> bool {
        self.kind == current.kind
            && self.palette_layout == current.palette_layout
            && self.bones.len() == current.bones.len()
            && self.deformation.is_some() == current.deformation.is_some()
    }

    fn draw_view(&self) -> DrawSnapshotView {
        DrawSnapshotView {
            world: &self.world,
            bones: PaletteView {
                data: self.bones.as_ptr(),
                count: self.bones.len(),
            },
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
struct FrameIdentity {
    source: u64,
    generation: u64,
    width: u32,
    height: u32,
}

impl FrameIdentity {
    fn follows(self, previous: Self) -> bool {
        // Source counters may skip between draws of a given view.
        self.source > previous.source
            && self.generation == previous.generation
            && self.width == previous.width
            && self.height == previous.height
    }
}

#[derive(Default)]
struct CameraSlot {
    sample: Option<CameraSample>,
    conflicted: bool,
}

struct Frame {
    id: FrameIdentity,
    cameras: [CameraSlot; 2],
    // A vacant slot has no draw. A present slot with None records a conflict.
    instances: BTreeMap<(InstanceKey, u8), Option<Snapshot<'static>>>,
}

#[derive(Default)]
struct View {
    current: Option<Frame>,
    previous: Option<Frame>,
}

#[derive(Default)]
struct MotionHistoryState {
    views: [View; 2],
    retired: BTreeMap<(u64, u64), u64>,
    conflicts: u64,
}

impl MotionHistoryState {
    fn begin(&mut self, view: usize, id: FrameIdentity) -> bool {
        let state = &mut self.views[view];
        if let Some(current) = &state.current {
            if id == current.id {
                return state.previous.is_some();
            }
            state.previous = if id.follows(current.id) {
                state.current.take()
            } else {
                None
            };
        }
        state.current = Some(Frame {
            id,
            cameras: Default::default(),
            instances: BTreeMap::new(),
        });
        state.previous.is_some()
    }

    fn observe(&mut self, view: usize, domain: usize, sample: &CameraSample) -> CameraStatus {
        let Some(frame) = &mut self.views[view].current else {
            return CameraStatus::NoFrame;
        };
        let slot = &mut frame.cameras[domain];
        if slot.conflicted {
            return CameraStatus::Conflict;
        }
        match &slot.sample {
            None => {
                slot.sample = Some(*sample);
                CameraStatus::Recorded
            }
            Some(old) if old == sample => CameraStatus::Same,
            Some(_) => {
                slot.sample = None;
                slot.conflicted = true;
                self.conflicts += 1;
                CameraStatus::Conflict
            }
        }
    }

    fn previous_camera(&self, view: usize, domain: usize) -> Option<&CameraSample> {
        let view = &self.views[view];
        view.current.as_ref()?;
        view.previous.as_ref()?.cameras[domain].sample.as_ref()
    }

    fn record(
        &mut self,
        view: usize,
        key: InstanceKey,
        domain: u8,
        snapshot: Snapshot<'_>,
    ) -> Option<&Snapshot<'static>> {
        if !key.valid()
            || self
                .retired
                .get(&key.identity())
                .is_some_and(|watermark| key.generation <= *watermark)
        {
            return None;
        }
        let view = &mut self.views[view];
        let slot = (key, domain);
        let current = view.current.as_mut()?;
        let recorded = match current.instances.entry(slot) {
            Entry::Vacant(entry) => entry.insert(Some(snapshot.into_owned())).as_ref()?,
            Entry::Occupied(entry) => {
                let stored = entry.into_mut();
                if *stored.as_ref()? != snapshot {
                    *stored = None;
                    self.conflicts += 1;
                    return None;
                }
                stored.as_ref()?
            }
        };
        let previous = view.previous.as_ref()?.instances.get(&slot)?.as_ref()?;
        previous.compatible(recorded).then_some(previous)
    }

    fn retire(&mut self, key: InstanceKey) {
        if !key.valid() {
            return;
        }
        let watermark = self.retired.entry(key.identity()).or_default();
        *watermark = (*watermark).max(key.generation);
        for view in &mut self.views {
            for frame in [&mut view.current, &mut view.previous]
                .into_iter()
                .flatten()
            {
                frame.instances.retain(|(key_in_frame, _), _| {
                    key_in_frame.identity() != key.identity()
                        || key_in_frame.generation > *watermark
                });
            }
        }
    }
}

// These flags come from the native ViewId and ProjectionDomain enums.
fn lane(id: u8) -> usize {
    usize::from(id == 1)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_history_create() -> *mut MotionHistoryState {
    Box::into_raw(Box::new(MotionHistoryState::default()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_history_destroy(state: *mut MotionHistoryState) {
    if !state.is_null() {
        drop(unsafe { Box::from_raw(state) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_begin(
    state: *mut MotionHistoryState,
    view: u8,
    source: u64,
    generation: u64,
    width: u32,
    height: u32,
) -> bool {
    unsafe { &mut *state }.begin(
        lane(view),
        FrameIdentity {
            source,
            generation,
            width,
            height,
        },
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_active(state: *const MotionHistoryState, view: u8) -> bool {
    unsafe { state.as_ref() }.is_some_and(|state| state.views[lane(view)].current.is_some())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_observe(
    state: *mut MotionHistoryState,
    view: u8,
    domain: u8,
    sample: &CameraSample,
) -> CameraStatus {
    unsafe { state.as_mut() }.map_or(CameraStatus::NoFrame, |state| {
        state.observe(lane(view), lane(domain), sample)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_camera(
    state: *const MotionHistoryState,
    view: u8,
    domain: u8,
) -> *const CameraSample {
    unsafe { state.as_ref() }
        .and_then(|state| state.previous_camera(lane(view), lane(domain)))
        .map_or(ptr::null(), ptr::from_ref)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_record(
    state: *mut MotionHistoryState,
    view: u8,
    key: &InstanceKey,
    input: &SnapshotInput,
    out: &mut MaybeUninit<DrawSnapshotView>,
) {
    let previous = unsafe { state.as_mut() }.and_then(|state| {
        let snapshot = unsafe { Snapshot::borrow(input) };
        state.record(lane(view), *key, input.domain, snapshot)
    });
    out.write(previous.map_or_else(DrawSnapshotView::default, Snapshot::draw_view));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_retire(state: *mut MotionHistoryState, key: &InstanceKey) {
    unsafe { &mut *state }.retire(*key);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_reset(state: *mut MotionHistoryState) {
    if let Some(state) = unsafe { state.as_mut() } {
        // Resource and shader resets leave retirement watermarks intact.
        state.views = Default::default();
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_conflicts(state: *const MotionHistoryState) -> u64 {
    unsafe { state.as_ref() }.map_or(0, |state| state.conflicts)
}

// b13 offsets are bytes, as reflected in the core and foliage shader families.
const CORE_MAX_BONES: usize = 78;
const CORE_BONES: usize = 64;
const CORE_HISTORY: usize = CORE_BONES + CORE_MAX_BONES * 48;
pub(crate) const CORE_BYTES: usize = CORE_HISTORY + 16;
const TESS_HISTORY: usize = 128;
pub(crate) const TESS_BYTES: usize = TESS_HISTORY + 16;
const FOLIAGE_MAX_GROUPS: usize = 61;
const FOLIAGE_HISTORY: usize = 160;
const FOLIAGE_ROWS: usize = 176;
const FOLIAGE_ITEM_STATE: usize = FOLIAGE_ROWS + FOLIAGE_MAX_GROUPS * 64;
pub(crate) const FOLIAGE_BYTES: usize = FOLIAGE_ITEM_STATE + FOLIAGE_MAX_GROUPS * 16;
pub(crate) const PIXEL_BYTES: usize = 48;
const SHADER_ABI: f32 = 1.;

fn put(out: &mut [u8], offset: usize, word: [u8; 4]) {
    out[offset..offset + 4].copy_from_slice(&word);
}

fn put_floats(out: &mut [u8], offset: usize, values: &[f32]) {
    for (i, value) in values.iter().enumerate() {
        put(out, offset + i * 4, value.to_le_bytes());
    }
}

fn put_matrix(out: &mut [u8], offset: usize, mat: &Mat4, rows: usize) {
    for (i, value) in mat.0[..rows * 4].iter().enumerate() {
        put(out, offset + i * 4, (*value as f32).to_le_bytes());
    }
}

fn initialized_invalid<const N: usize>(
    out: &mut MaybeUninit<[u8; N]>,
    history: usize,
) -> &mut [u8; N] {
    let out = out.write([0; N]);
    put(out, history + 12, SHADER_ABI.to_le_bytes());
    out
}

#[derive(Default)]
#[repr(C)]
struct CoreHistory {
    draw: bool,
    palette: bool,
    rigid: bool,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_core_invalid(out: &mut MaybeUninit<[u8; CORE_BYTES]>) {
    initialized_invalid(out, CORE_HISTORY);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_pack_core(
    wvp: &Mat4,
    bones: *const Mat4,
    count: usize,
    history: &CoreHistory,
    out: &mut MaybeUninit<[u8; CORE_BYTES]>,
) -> bool {
    let out = initialized_invalid(out, CORE_HISTORY);
    if count > CORE_MAX_BONES || !wvp.usable() {
        return false;
    }
    let bones = unsafe { borrow_span(bones, count) };
    if bones.iter().any(|bone| !bone.usable()) {
        return false;
    }
    put_matrix(out, 0, wvp, 4);
    for (i, bone) in bones.iter().enumerate() {
        put_matrix(out, CORE_BONES + i * 48, bone, 3);
    }
    put_floats(
        out,
        CORE_HISTORY,
        &[
            history.draw as u8 as f32,
            history.palette as u8 as f32,
            history.rigid as u8 as f32,
        ],
    );
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_tess_invalid(out: &mut MaybeUninit<[u8; TESS_BYTES]>) {
    initialized_invalid(out, TESS_HISTORY);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_tess(
    wv: &Mat4,
    projection: &Mat4,
    valid: bool,
    out: &mut MaybeUninit<[u8; TESS_BYTES]>,
) -> bool {
    let out = initialized_invalid(out, TESS_HISTORY);
    if !valid || !wv.usable() || !projection.usable() {
        return false;
    }
    put_matrix(out, 0, wv, 4);
    put_matrix(out, 64, projection, 4);
    put(out, TESS_HISTORY, 1.0f32.to_le_bytes());
    true
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct FoliageConstants {
    consts: [f32; 4],
    wave: [f32; 4],
    wind: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FoliageItem {
    valid: bool,
    scale: f32,
    fade: f32,
    rows: [f32; 16],
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn foliage_common(out: &mut [u8], clip: &Mat4, constants: &FoliageConstants, groups: u32) -> bool {
    if !clip.usable()
        || !finite(&constants.consts)
        || !finite(&constants.wave)
        || !finite(&constants.wind)
    {
        return false;
    }
    put_matrix(out, 0, clip, 4);
    put_floats(out, 112, &constants.consts);
    put_floats(out, 128, &constants.wave);
    put_floats(out, 144, &constants.wind);
    put(out, FOLIAGE_HISTORY, 1u32.to_le_bytes());
    put(out, FOLIAGE_HISTORY + 4, groups.to_le_bytes());
    put(out, FOLIAGE_HISTORY + 8, 1u32.to_le_bytes());
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_foliage_invalid(out: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>) {
    out.write([0; FOLIAGE_BYTES]);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_tree(
    clip: &Mat4,
    xform: &[f32; 12],
    constants: &FoliageConstants,
    out: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>,
) -> bool {
    let out = out.write([0; FOLIAGE_BYTES]);
    if !finite(xform) || !foliage_common(out, clip, constants, 0) {
        return false;
    }
    put_floats(out, 64, xform);
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_pack_detail(
    clip: &Mat4,
    constants: &FoliageConstants,
    items: *const FoliageItem,
    count: usize,
    groups: u32,
    out: &mut MaybeUninit<[u8; FOLIAGE_BYTES]>,
) -> bool {
    let out = out.write([0; FOLIAGE_BYTES]);
    let live = groups as usize;
    if live > FOLIAGE_MAX_GROUPS || live > count || !foliage_common(out, clip, constants, groups) {
        return false;
    }
    for (g, item) in unsafe { borrow_span(items, live) }.iter().enumerate() {
        // Unmatched groups keep the zero state and never read the row data.
        if item.valid
            && finite(&item.rows)
            && item.scale > 0.
            && item.scale.is_finite()
            && item.fade > 0.
            && item.fade <= 1.
        {
            put_floats(out, FOLIAGE_ROWS + g * 64, &item.rows);
            put_floats(
                out,
                FOLIAGE_ITEM_STATE + g * 16,
                &[1., item.scale, item.fade, 0.],
            );
        }
    }
    true
}

#[repr(C)]
struct PixelInputs {
    viewport: [f32; 4],
    jitter: [f32; 4],
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_pack_pixel(
    input: &PixelInputs,
    out: &mut MaybeUninit<[u8; PIXEL_BYTES]>,
) -> bool {
    let out = out.write([0; PIXEL_BYTES]);
    let [x, y, width, height] = input.viewport;
    if !finite(&input.viewport) || !finite(&input.jitter) || width <= 0. || height <= 0. {
        return false;
    }
    let viewport = [x, y, width.recip(), height.recip()];
    if !finite(&viewport) {
        return false;
    }
    put_floats(out, 0, &viewport);
    put_floats(out, 16, &input.jitter);
    put(out, 32, SHADER_ABI.to_le_bytes());
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_remove_jitter(
    projection: &Mat4,
    u: f32,
    v: f32,
    out: &mut MaybeUninit<Mat4>,
) {
    out.write(projection.remove_jitter(u, v));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_from_engine(values: &[f32; 16], out: &mut MaybeUninit<Mat4>) {
    // Transpose native row-vector Fmatrix bytes for column-vector shader math.
    out.write(Mat4(std::array::from_fn(|i| {
        f64::from(values[i % 4 * 4 + i / 4])
    })));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_compose(a: &Mat4, b: &Mat4, out: &mut MaybeUninit<Mat4>) {
    out.write(a.compose(b));
}

#[derive(Default, PartialEq)]
#[repr(u8)]
enum HistoryReason {
    Valid = 0,
    #[default]
    NoVisual = 1,
    Unidentified = 2,
    NoContext = 3,
    UnsupportedKind = 4,
    KindMismatch = 5,
    NoPreviousCamera = 6,
    CameraConflict = 7,
    NoPreviousInstance = 8,
    TooManyBones = 9,
    #[cfg(windows)]
    Foliage = 10,
    FoliageNoConstants = 11,
    FoliageNoPrevious = 12,
    FoliageLodChanged = 13,
    FoliageXformChanged = 14,
    FoliageWorld = 15,
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_history_reason_name(reason: u8) -> *const c_char {
    const NAMES: [&std::ffi::CStr; 16] = [
        c"valid",
        c"no_visual",
        c"unidentified",
        c"no_context",
        c"unsupported_kind",
        c"kind_mismatch",
        c"no_previous_camera",
        c"camera_conflict",
        c"no_previous_instance",
        c"too_many_bones",
        c"foliage",
        c"foliage_no_constants",
        c"foliage_no_previous",
        c"foliage_lod_changed",
        c"foliage_xform_changed",
        c"foliage_world",
    ];
    NAMES
        .get(reason as usize)
        .copied()
        .unwrap_or(c"unknown")
        .as_ptr()
}

const RIGID: u8 = 0;
const HARDWARE_SKINNED: u8 = 1;
const SINGLE_BONE: u8 = 2;
const TREE: u8 = 4;

const IDENTIFIED: u8 = 0;
const STATIC_WORLD: u8 = 2;
const IDENTIFIED_FOLIAGE: u8 = 8;

#[repr(C)]
struct CoreDrawInput {
    family: u8,
    skin: i32,
    in_visual: bool,
    verdict: u8,
    kind: u8,
    context: bool,
    camera_conflict: bool,
    previous_camera: *const CameraSample,
    previous: DrawSnapshotView,
}

#[derive(Default)]
#[repr(C)]
struct CoreDecision {
    reason: HistoryReason,
    flags: CoreHistory,
    previous_wvp: Mat4,
    previous_wv: Mat4,
    previous_projection: Mat4,
    bones: PaletteView,
}

unsafe fn decide_core(input: &CoreDrawInput) -> Result<CoreDecision, HistoryReason> {
    use HistoryReason::*;
    if !input.in_visual {
        return Err(NoVisual);
    }
    if !matches!(input.verdict, IDENTIFIED | STATIC_WORLD) {
        return Err(Unidentified);
    }
    if !input.context {
        return Err(NoContext);
    }
    let rigid_model = input.family == CORE_MODEL && input.skin == -1;
    let rigid = input.family == CORE_STATIC || rigid_model;
    match (input.verdict, input.kind) {
        (STATIC_WORLD, _) | (_, RIGID) if rigid => {}
        (STATIC_WORLD, _) | (_, RIGID) => return Err(KindMismatch),
        (_, HARDWARE_SKINNED) if input.family == CORE_MODEL && (1..=4).contains(&input.skin) => {}
        // SKIN_0 has the bone composed into the engine's per-draw world.
        (_, SINGLE_BONE) if input.family == CORE_MODEL && input.skin == 0 => {}
        (_, HARDWARE_SKINNED | SINGLE_BONE) => return Err(KindMismatch),
        _ => return Err(UnsupportedKind),
    }
    if input.camera_conflict {
        return Err(CameraConflict);
    }
    let camera = unsafe { input.previous_camera.as_ref() }.ok_or(NoPreviousCamera)?;
    let mut bones = PaletteView::default();
    let world = if input.verdict == IDENTIFIED {
        let world = unsafe { input.previous.world.as_ref() }.ok_or(NoPreviousInstance)?;
        if input.kind == HARDWARE_SKINNED {
            bones = input.previous.bones;
            if bones.count == 0 {
                return Err(NoPreviousInstance);
            }
            if bones.count > CORE_MAX_BONES {
                return Err(TooManyBones);
            }
        }
        world
    } else {
        &Mat4::IDENTITY
    };
    Ok(CoreDecision {
        reason: Valid,
        flags: CoreHistory {
            draw: true,
            palette: bones.count != 0,
            rigid: rigid_model,
        },
        previous_wvp: camera.projection.compose(&camera.view).compose(world),
        previous_wv: camera.view.compose(world),
        previous_projection: camera.projection,
        bones,
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_decide_core(
    input: &CoreDrawInput,
    out: &mut MaybeUninit<CoreDecision>,
) {
    let decision = unsafe { decide_core(input) }.unwrap_or_else(|reason| CoreDecision {
        reason,
        ..Default::default()
    });
    out.write(decision);
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
struct TreeRecord {
    xform: [f32; 12],
    constants: FoliageConstants,
    lod: i64,
}

#[repr(C)]
struct TreeDrawInput {
    family: u8,
    in_visual: bool,
    kind: u8,
    verdict: u8,
    context: bool,
    camera_conflict: bool,
    previous_camera: *const CameraSample,
    constants_ok: bool,
    current: TreeRecord,
    previous: *const TreeRecord,
    previous_jitter_u: f32,
    previous_jitter_v: f32,
}

#[repr(C)]
struct DetailDrawInput {
    family: u8,
    detail_window: bool,
    context: bool,
    camera_conflict: bool,
    previous_camera: *const CameraSample,
    constants_ok: bool,
    world_readable: bool,
    world_identity: bool,
    route_found: bool,
    live_groups: u32,
    previous_jitter_u: f32,
    previous_jitter_v: f32,
}

#[repr(C)]
struct FoliageDecision {
    reason: HistoryReason,
    previous_clip: Mat4,
}

impl From<Result<Mat4, HistoryReason>> for FoliageDecision {
    fn from(result: Result<Mat4, HistoryReason>) -> Self {
        match result {
            Ok(previous_clip) => Self {
                reason: HistoryReason::Valid,
                previous_clip,
            },
            Err(reason) => Self {
                reason,
                previous_clip: Mat4::default(),
            },
        }
    }
}

fn foliage_clip(camera: &CameraSample, u: f32, v: f32) -> Mat4 {
    camera.projection.remove_jitter(u, v).compose(&camera.view)
}

unsafe fn decide_tree(input: &TreeDrawInput) -> Result<Mat4, HistoryReason> {
    use HistoryReason::*;
    if input.family != FOLIAGE_TREE {
        return Err(KindMismatch);
    }
    if !input.in_visual || input.kind != TREE {
        return Err(NoVisual);
    }
    if !matches!(input.verdict, IDENTIFIED_FOLIAGE | IDENTIFIED) {
        return Err(Unidentified);
    }
    if !input.context {
        return Err(NoContext);
    }
    if !input.constants_ok {
        return Err(FoliageNoConstants);
    }
    if input.camera_conflict {
        return Err(CameraConflict);
    }
    let camera = unsafe { input.previous_camera.as_ref() }.ok_or(NoPreviousCamera)?;
    let previous = unsafe { input.previous.as_ref() }.ok_or(FoliageNoPrevious)?;
    // The same level tree's staged transform stays bitwise identical.
    if previous.xform.map(f32::to_bits) != input.current.xform.map(f32::to_bits) {
        return Err(FoliageXformChanged);
    }
    if previous.lod != input.current.lod {
        return Err(FoliageLodChanged);
    }
    Ok(foliage_clip(
        camera,
        input.previous_jitter_u,
        input.previous_jitter_v,
    ))
}

unsafe fn decide_detail(input: &DetailDrawInput) -> Result<Mat4, HistoryReason> {
    use HistoryReason::*;
    if input.family != FOLIAGE_DETAIL {
        return Err(KindMismatch);
    }
    if !input.detail_window {
        return Err(NoVisual);
    }
    if !input.context {
        return Err(NoContext);
    }
    if !input.constants_ok || input.live_groups == 0 {
        return Err(FoliageNoConstants);
    }
    // The detail transform already operates on world-space positions.
    if !input.world_readable || !input.world_identity {
        return Err(FoliageWorld);
    }
    if input.camera_conflict {
        return Err(CameraConflict);
    }
    let camera = unsafe { input.previous_camera.as_ref() }.ok_or(NoPreviousCamera)?;
    if !input.route_found {
        return Err(FoliageNoPrevious);
    }
    Ok(foliage_clip(
        camera,
        input.previous_jitter_u,
        input.previous_jitter_v,
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_decide_tree(
    input: &TreeDrawInput,
    out: &mut MaybeUninit<FoliageDecision>,
) {
    out.write(unsafe { decide_tree(input) }.into());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_decide_detail(
    input: &DetailDrawInput,
    out: &mut MaybeUninit<FoliageDecision>,
) {
    out.write(unsafe { decide_detail(input) }.into());
}

const DISTANCE_BANDS: [&std::ffi::CStr; 6] = [
    c"0-20m",
    c"20-50m",
    c"50-100m",
    c"100-150m",
    c"150m+",
    c"unknown",
];

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_distance_band(metres: f64) -> usize {
    if !metres.is_finite() || metres < 0. {
        DISTANCE_BANDS.len() - 1
    } else {
        [20., 50., 100., 150.]
            .iter()
            .take_while(|limit| metres >= **limit)
            .count()
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_distance_band_name(band: usize) -> *const c_char {
    DISTANCE_BANDS
        .get(band)
        .copied()
        .unwrap_or(c"unknown")
        .as_ptr()
}

fn distance_squared(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [dx, dy, dz] = std::array::from_fn(|i| a[i] - b[i]);
    dx * dx + dy * dy + dz * dz
}

fn translation(matrix: &Mat4) -> [f64; 3] {
    [matrix.0[3], matrix.0[7], matrix.0[11]]
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_camera_distance(view: &Mat4, world: &Mat4) -> f64 {
    let v = &view.0;
    if v[12..] != [0., 0., 0., 1.] {
        return f64::NAN;
    }
    // [R t; 0 1] maps world to view, so the position is -R^T t.
    let camera = std::array::from_fn(|c| -(v[c] * v[3] + v[4 + c] * v[7] + v[8 + c] * v[11]));
    distance_squared(translation(world), camera).sqrt()
}

#[repr(u8)]
enum ModelElement {
    Unknown,
    High,
    Low,
    Tessellated,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_model_element(
    root: *const u8,
    count: usize,
) -> ModelElement {
    let root = unsafe { borrow_span(root, count) };
    if !root.starts_with(b"deffer_model") {
        ModelElement::Unknown
    } else if root.windows(5).any(|part| part == b"(tess") {
        ModelElement::Tessellated
    } else if root.windows(3).any(|part| part == b"-hq") {
        ModelElement::High
    } else {
        ModelElement::Low
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_stale_palette(
    previous: &DrawSnapshotView,
    current: &DrawSnapshotView,
    min_move: f64,
) -> bool {
    if current.bones.count == 0 || previous.bones.count != current.bones.count {
        return false;
    }
    let (Some(previous_world), Some(current_world)) =
        (unsafe { previous.world.as_ref() }, unsafe {
            current.world.as_ref()
        })
    else {
        return false;
    };
    let old = unsafe { borrow_span(previous.bones.data, previous.bones.count) };
    let new = unsafe { borrow_span(current.bones.data, current.bones.count) };
    old.iter()
        .zip(new)
        .all(|(a, b)| a.0.map(f64::to_bits) == b.0.map(f64::to_bits))
        && distance_squared(translation(current_world), translation(previous_world))
            > min_move * min_move
}
