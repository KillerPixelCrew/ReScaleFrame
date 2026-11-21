// SPDX-License-Identifier: MIT
//! Presentation snapshots and the NGX observations belonging to one admitted capture.
use super::{Capture, rsf_rdr2_capture_has_inputs};
use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::{from_ref, null};
use std::sync::atomic::Ordering;

const MAX_HDR_BYTES: usize = 1024;
const CAMERA_BYTES: usize = 0x800;
const SCALAR_KEYS: [&CStr; 18] = [
    c"Jitter.Offset.X",
    c"Jitter.Offset.Y",
    c"MV.Scale.X",
    c"MV.Scale.Y",
    c"DLSS.Pre.Exposure",
    c"DLSS.Exposure.Scale",
    c"FrameTimeDeltaInMsec",
    c"Reset",
    c"DLSS.Render.Subrect.Dimensions.Width",
    c"DLSS.Render.Subrect.Dimensions.Height",
    c"DLSS.Input.Color.Subrect.Base.X",
    c"DLSS.Input.Color.Subrect.Base.Y",
    c"DLSS.Input.Depth.Subrect.Base.X",
    c"DLSS.Input.Depth.Subrect.Base.Y",
    c"DLSS.Input.MV.Subrect.Base.X",
    c"DLSS.Input.MV.Subrect.Base.Y",
    c"DLSS.Output.Subrect.Base.X",
    c"DLSS.Output.Subrect.Base.Y",
];
const FLOAT_SCALARS: usize = 7;
const OUTPUT: usize = 4;
const RESOURCE_COUNT: usize = OUTPUT + 1;
#[cfg(windows)]
const RESOURCE_ROLES: [&str; RESOURCE_COUNT] = ["color", "depth", "motion", "exposure", "output"];

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct PresentationDesc {
    pub(super) format: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

#[derive(Clone, Copy)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) struct Hdr {
    pub(super) kind: u32,
    len: usize,
    bytes: [u8; MAX_HDR_BYTES],
}

#[cfg(windows)]
impl Hdr {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct PresentationDetails {
    pub(super) color_space: Option<u32>,
    pub(super) hdr: Option<Hdr>,
}

#[derive(Default)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(super) struct Presentation {
    pub(super) descriptor: PresentationDesc,
    pub(super) details: PresentationDetails,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct InputId {
    pub(super) sequence: u32,
    pub(super) index: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct CameraFacts {
    pub(super) frame: u32,
    pub(super) thread: u32,
    pub(super) tick: u64,
    pub(super) qpc: u64,
    pub(super) view: u64,
    pub(super) draw_context: u64,
    pub(super) job: u64,
}

const _: () = {
    assert!(size_of::<PresentationDesc>() == 12);
    assert!(size_of::<InputId>() == 8);
    assert!(size_of::<CameraFacts>() == 48 && std::mem::offset_of!(CameraFacts, tick) == 8);
};

#[derive(Clone, Copy)]
enum ResourceRefusal {
    Missing,
    CopyUnavailable,
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(super) struct Temporal {
    pub(super) id: InputId,
    pub(super) feature_flags: u32,
    pub(super) list: usize,
    pub(super) generation: u64,
    pub(super) facts: CameraFacts,
    pub(super) camera: Option<[u8; CAMERA_BYTES]>,
    pub(super) result: Option<u64>,
    scalars: [Option<f64>; SCALAR_KEYS.len()],
    resources: [Option<ResourceRefusal>; RESOURCE_COUNT],
}

#[cfg(windows)]
impl Temporal {
    pub(super) fn scalars(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.scalars
            .iter()
            .zip(SCALAR_KEYS)
            .filter_map(|(value, key)| Some((key.to_str().ok()?, (*value)?)))
    }

    pub(super) fn refusals(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        let camera = self.camera.is_none().then_some(("camera_unreadable", ""));
        let scalars = self
            .scalars
            .iter()
            .zip(SCALAR_KEYS)
            .filter_map(|(value, key)| {
                if value.is_none() {
                    key.to_str().ok()
                } else {
                    None
                }
            })
            .map(|key| ("scalar_unavailable:", key));
        let resources = self
            .resources
            .iter()
            .zip(RESOURCE_ROLES)
            .filter_map(|(refusal, role)| {
                Some((
                    role,
                    match refusal.as_ref()? {
                        ResourceRefusal::Missing => "_missing",
                        ResourceRefusal::CopyUnavailable => "_state_or_copy_unavailable",
                    },
                ))
            });
        let evaluation = self
            .result
            .filter(|result| *result as u32 != 1)
            .map(|_| ("native_evaluation_failed", ""));
        camera
            .into_iter()
            .chain(scalars)
            .chain(resources)
            .chain(evaluation)
    }
}

#[derive(Default)]
pub(super) struct Metadata {
    swapchains: HashMap<usize, PresentationDetails>,
    features: HashMap<usize, u32>,
    pub(super) presentation: Presentation,
    pub(super) temporal: Vec<Temporal>,
}

impl Metadata {
    pub(super) fn arm(&mut self, identity: usize, descriptor: PresentationDesc) {
        self.presentation = Presentation {
            descriptor,
            details: self.swapchains.get(&identity).copied().unwrap_or_default(),
        };
        self.temporal.clear();
    }

    fn swapchain(&mut self, identity: usize) -> Option<&mut PresentationDetails> {
        if !self.swapchains.contains_key(&identity) && self.swapchains.try_reserve(1).is_err() {
            return None;
        }
        Some(self.swapchains.entry(identity).or_default())
    }

    fn input(&mut self, id: InputId) -> Option<&mut Temporal> {
        self.temporal.iter_mut().find(|input| input.id == id)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_reset_presentation(owner: &Capture, identity: *mut c_void) {
    unsafe { &mut *owner.metadata.get() }
        .swapchains
        .remove(&identity.addr());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_record_color_space(
    owner: &Capture,
    identity: *mut c_void,
    color_space: u32,
) {
    if let Some(details) = unsafe { &mut *owner.metadata.get() }.swapchain(identity.addr()) {
        details.color_space = Some(color_space);
    }
}

/// DXGI lends bytes for the successful SetHDRMetaData call. The metadata type is recorded
/// even when the payload is empty or exceeds the capture's bound.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_record_hdr(
    owner: &Capture,
    identity: *mut c_void,
    kind: u32,
    bytes: *const u8,
    len: usize,
) {
    if let Some(details) = unsafe { &mut *owner.metadata.get() }.swapchain(identity.addr()) {
        let mut hdr = Hdr {
            kind,
            len: 0,
            bytes: [0; MAX_HDR_BYTES],
        };
        if !bytes.is_null() && len <= MAX_HDR_BYTES {
            hdr.bytes[..len].copy_from_slice(unsafe { std::slice::from_raw_parts(bytes, len) });
            hdr.len = len;
        }
        details.hdr = Some(hdr);
    }
}

/// A successful release or a new creation without verified SR flags invalidates an old handle.
/// A pointer to zero flags records a known feature.
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_record_feature(
    owner: &Capture,
    handle: *mut c_void,
    flags: *const u32,
) {
    let features = &mut unsafe { &mut *owner.metadata.get() }.features;
    let handle = handle.addr();
    if let Some(&flags) = unsafe { flags.as_ref() } {
        if features.contains_key(&handle) || features.try_reserve(1).is_ok() {
            features.insert(handle, flags);
        }
    } else {
        features.remove(&handle);
    }
}

#[derive(Clone, Copy)]
struct ActiveInput {
    owner: *const Capture,
    depth: usize,
    id: InputId,
    committed: bool,
    ending: bool,
}

#[derive(Clone, Copy)]
struct Evaluation {
    depth: usize,
    active: Option<ActiveInput>,
}

thread_local! {
    static EVALUATION: Cell<Evaluation> = const { Cell::new(Evaluation { depth: 0, active: None }) };
}

/// The census calls both phases with the same arguments on the same thread. Count even
/// skipped calls so a nested exit cannot complete an outer capture on the same list.
#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_rdr2_capture_track_evaluation(ending: bool) -> bool {
    EVALUATION.with(|thread| {
        let mut evaluation = thread.get();
        let eligible = if ending {
            let depth = evaluation.depth;
            let matched = evaluation.active.as_mut().is_some_and(|input| {
                if input.ending || input.depth != depth {
                    return false;
                }
                input.ending = true;
                true
            });
            evaluation.depth = evaluation.depth.saturating_sub(1);
            matched
        } else {
            evaluation.depth += 1;
            evaluation.active.is_none()
        };
        thread.set(evaluation);
        eligible
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_begin_input(
    owner: &Capture,
    list: *mut c_void,
    handle: *mut c_void,
    id: &mut InputId,
) -> bool {
    let ledger = unsafe { &mut *owner.ledger.get() };
    if list.is_null() || !ledger.dirty || !rsf_rdr2_capture_has_inputs(owner) {
        return false;
    }
    EVALUATION.with(|thread| {
        let mut evaluation = thread.get();
        if evaluation.active.is_some() || evaluation.depth == 0 {
            return false;
        }
        let metadata = unsafe { &mut *owner.metadata.get() };
        let Some(&feature_flags) = metadata.features.get(&handle.addr()) else {
            return false;
        };
        if metadata.temporal.try_reserve(1).is_err() {
            return false;
        }
        *id = InputId {
            sequence: ledger.sequence,
            index: ledger.input_index,
        };
        ledger.input_index = ledger.input_index.wrapping_add(1);
        let list = list.addr();
        let generation = unsafe { &*owner.recordings.get() }.generation(list);
        metadata.temporal.push(Temporal {
            id: *id,
            feature_flags,
            list,
            generation,
            facts: CameraFacts::default(),
            camera: None,
            result: None,
            scalars: [None; SCALAR_KEYS.len()],
            resources: [None; RESOURCE_COUNT],
        });
        evaluation.active = Some(ActiveInput {
            owner: from_ref(owner),
            depth: evaluation.depth,
            id: *id,
            committed: false,
            ending: false,
        });
        thread.set(evaluation);
        true
    })
}

/// Commit only after native recording of the input copies. A refused recording leaves the
/// slot available while preserving the reserved index of any copies already recorded.
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_commit_input(owner: &Capture) -> bool {
    EVALUATION.with(|thread| {
        let mut evaluation = thread.get();
        let Some(input) = evaluation.active.as_mut() else {
            return false;
        };
        let ledger = unsafe { &mut *owner.ledger.get() };
        if input.owner != from_ref(owner) || input.id.sequence != ledger.sequence || input.committed
        {
            return false;
        }
        let remaining = owner.inputs_remaining.load(Ordering::Acquire);
        if remaining == 0 {
            return false;
        }
        input.committed = true;
        thread.set(evaluation);
        ledger.inputs_in_flight += 1;
        owner
            .inputs_remaining
            .store(remaining - 1, Ordering::Release);
        true
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_rdr2_capture_finish_input(owner: &Capture) {
    EVALUATION.with(|thread| {
        let mut evaluation = thread.get();
        let Some(input) = evaluation
            .active
            .filter(|input| input.owner == from_ref(owner))
        else {
            return;
        };
        evaluation.active = None;
        thread.set(evaluation);
        let ledger = unsafe { &mut *owner.ledger.get() };
        if input.id.sequence != ledger.sequence {
            return;
        }
        if input.committed {
            ledger.inputs_in_flight -= 1;
        } else {
            let metadata = unsafe { &mut *owner.metadata.get() };
            metadata.temporal.retain(|record| record.id != input.id);
        }
    });
}

/// The native observer lends a full readable camera sample or null if its read failed.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_input_camera(
    owner: &Capture,
    id: &InputId,
    facts: &CameraFacts,
    bytes: *const u8,
) {
    if let Some(input) = unsafe { &mut *owner.metadata.get() }.input(*id) {
        input.facts = *facts;
        input.camera = unsafe { bytes.cast::<[u8; CAMERA_BYTES]>().as_ref() }.copied();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_rdr2_capture_input_scalar_key(index: u32, floating: &mut bool) -> *const c_char {
    let index = index as usize;
    *floating = index < FLOAT_SCALARS;
    SCALAR_KEYS.get(index).map_or(null(), |key| key.as_ptr())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_input_scalar(
    owner: &Capture,
    id: &InputId,
    index: u32,
    value: f64,
    available: bool,
) {
    if let Some(scalar) = unsafe { &mut *owner.metadata.get() }
        .input(*id)
        .and_then(|input| input.scalars.get_mut(index as usize))
    {
        *scalar = (available && value.is_finite()).then_some(value);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_input_resource_refused(
    owner: &Capture,
    id: &InputId,
    role: u32,
    missing: bool,
) {
    if let Some(refusal) = unsafe { &mut *owner.metadata.get() }
        .input(*id)
        .and_then(|input| input.resources.get_mut(role as usize))
    {
        *refusal = Some(if missing {
            ResourceRefusal::Missing
        } else {
            ResourceRefusal::CopyUnavailable
        });
    }
}

/// Preserve the exact native result even when the list recording no longer qualifies
/// for an output copy. The caller finishes the reservation after the copy attempt.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_input_result(
    owner: &Capture,
    list: *mut c_void,
    result: u64,
    id: &mut InputId,
) -> bool {
    let Some(active) = EVALUATION
        .with(|thread| thread.get().active)
        .filter(|input| input.owner == from_ref(owner) && input.committed && input.ending)
    else {
        return false;
    };
    let Some(input) = unsafe { &mut *owner.metadata.get() }.input(active.id) else {
        return false;
    };
    input.result = Some(result);
    if result as u32 != 1 {
        return false;
    }
    let list = list.addr();
    let generation = unsafe { &*owner.recordings.get() }.generation(list);
    if input.list != list || input.generation != generation {
        input.resources[OUTPUT] = Some(ResourceRefusal::CopyUnavailable);
        return false;
    }
    *id = active.id;
    true
}
