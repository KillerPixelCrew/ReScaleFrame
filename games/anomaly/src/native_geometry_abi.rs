// SPDX-License-Identifier: MIT
//! Private layouts mirrored in native_geometry_platform.h.

use crate::engine_guards::{NativeSite, Read, UniqueGuard};
use crate::ffi::borrow_span;
use crate::geometry_registry::IdentityRegistry;
use crate::geometry_registry_abi::{
    RSF_ANOMALY_DRAW_COUNT, RSF_ANOMALY_PASS_COUNT, RSF_ANOMALY_TYPE_COUNT,
    rsf_anomaly_geometry_key as Key,
};
use std::ffi::c_void;

pub(crate) const UNSET: u8 = 255;
pub(crate) const RIGID: u8 = 0;
pub(crate) const HARDWARE_SKINNED: u8 = 1;
pub(crate) const SINGLE_BONE: u8 = 2;
pub(crate) const SOFTWARE_SKINNED: u8 = 3;
pub(crate) const TREE: u8 = 4;
pub(crate) const MAIN: u8 = 0;
pub(crate) const SCOPE: u8 = 1;
pub(crate) const WORLD: u8 = 0;
pub(crate) const FIRST_PERSON: u8 = 1;

pub(crate) const PASSES: usize = RSF_ANOMALY_PASS_COUNT as usize;
pub(crate) const TYPES: usize = RSF_ANOMALY_TYPE_COUNT as usize;
pub(crate) const VERDICTS: usize = RSF_ANOMALY_DRAW_COUNT as usize;

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Engine {
    pub module: usize,
    pub profile: usize,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct ChildRange {
    pub first: usize,
    pub end: usize,
    pub capacity: usize,
}

#[repr(C)]
pub(crate) struct Hierarchy {
    pub vtable_rvas: [u32; 4],
    pub children_offset: u32,
    pub read: Read,
}

const _: () = {
    assert!(size_of::<Hierarchy>() == 32);
    assert!(std::mem::offset_of!(Hierarchy, read) == 24);
};

#[repr(C)]
pub(crate) struct Layout {
    pub phase: u32,
    pub viewport: u32,
    pub frame: u32,
    pub world: u32,
}

pub(crate) type NativeMatrix = [f32; 16];
pub(crate) type Matrix = [f64; 16];
pub(crate) type ReadBones = unsafe extern "C" fn(usize, usize, u32, *mut NativeMatrix) -> bool;

#[repr(C)]
pub(crate) struct Context {
    pub previous: *mut Context,
    pub view: u8,
    pub domain: u8,
}

#[repr(C)]
pub(crate) struct Window {
    pub previous: *mut Window,
    pub near_context: Context,
    pub near_active: bool,
}

#[repr(C)]
pub(crate) struct Visual {
    pub previous: *mut Visual,
    pub registry: *const IdentityRegistry,
    pub facade: *const c_void,
    pub key: Key,
    pub world: usize,
    pub skeleton: usize,
    pub visual: u64,
    pub pass: u8,
    pub visual_type: u8,
    pub verdict: u8,
    pub kind: u8,
    pub view: u8,
    pub domain: u8,
}

#[repr(C)]
pub(crate) struct Facts {
    pub visual: u64,
    pub tree_lod: i64,
    pub pass: u8,
    pub visual_pass: u8,
    pub visual_type: u8,
    pub verdict: u8,
    pub kind: u8,
    pub view: u8,
    pub domain: u8,
    pub in_visual: bool,
    pub detail: bool,
}

// The Windows x64 ABI passes each native const reference as a pointer.
// The backing C++ objects live through the synchronous callback.
pub(crate) enum NativeDrawCapture {}
pub(crate) enum NativeDrawFacts {}

pub(crate) type Retired = unsafe extern "C" fn(*mut c_void, *const Key);
pub(crate) type DrawSink = unsafe extern "C" fn(*mut c_void, *const NativeDrawCapture);
pub(crate) type Flush = unsafe extern "C" fn(*const NativeDrawFacts);
pub(crate) type Near = unsafe extern "C" fn(*mut c_void);

#[repr(C)]
pub(crate) struct Writer {
    pub begin: unsafe extern "C" fn(*mut c_void, *const Visual, *const Matrix, u32) -> *mut Matrix,
    pub finish: unsafe extern "C" fn(*mut c_void, u64) -> *const NativeDrawCapture,
}

/// The selected build's immutable guard facts. Each pointer borrows a static
/// profile table, and `read` is the guarded engine-memory leaf.
#[repr(C)]
pub(crate) struct GuardProfile {
    pub read: Read,
    pub layout_sites: *const NativeSite,
    pub layout_count: usize,
    pub sites: *const NativeSite,
    pub site_count: usize,
    pub unique: *const UniqueGuard,
    pub unique_count: usize,
    pub image_size: u32,
}

// SAFETY for each accessor: the native lender returns pointers and counts of
// static profile tables, so every borrowed slice lives for the process.
impl GuardProfile {
    pub fn layout_sites(&self) -> &[NativeSite] {
        unsafe { borrow_span(self.layout_sites, self.layout_count) }
    }

    pub fn sites(&self) -> &[NativeSite] {
        unsafe { borrow_span(self.sites, self.site_count) }
    }

    pub fn unique(&self) -> &[UniqueGuard] {
        unsafe { borrow_span(self.unique, self.unique_count) }
    }
}

#[repr(C)]
pub(crate) struct Platform {
    pub guard_profile: unsafe extern "C" fn(Engine) -> GuardProfile,
    pub created: unsafe extern "C" fn() -> bool,
    pub install: unsafe extern "C" fn(Engine, u32) -> bool,
    pub enable: unsafe extern "C" fn() -> bool,
    pub disable: unsafe extern "C" fn() -> bool,
    pub hierarchy: unsafe extern "C" fn(Engine) -> Hierarchy,
    pub layout: unsafe extern "C" fn(Engine) -> Layout,
    pub read: Read,
    pub bones: ReadBones,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct Stats {
    pub models_registered: u64,
    pub models_retired: u64,
    pub draws_identified: u64,
    pub draws_unidentified: u64,
    pub capture_faults: u64,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct Coverage {
    pub draws: [[[u64; VERDICTS]; TYPES]; PASSES],
    pub flush_total: [u64; PASSES],
    pub flush_in_visual: [u64; PASSES],
    pub captured: u64,
    pub capture_stale: u64,
    pub capture_no_world: u64,
    pub capture_no_palette: u64,
    pub capture_no_context: u64,
    pub sunk: u64,
    pub detail_renders: [u64; PASSES],
    pub flush_in_detail: [u64; PASSES],
    pub views_main: u64,
    pub views_scope: u64,
    pub hud_scopes: u64,
    pub near_windows: u64,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<Engine>() == 16);
    assert!(size_of::<ChildRange>() == 24);
    assert!(size_of::<Layout>() == 16);
    assert!(size_of::<Context>() == 16);
    assert!(size_of::<Window>() == 32);
    assert!(size_of::<Visual>() == 80);
    assert!(size_of::<Facts>() == 32);
    assert!(size_of::<Writer>() == 16);
    assert!(size_of::<GuardProfile>() == 64);
    assert!(std::mem::offset_of!(GuardProfile, sites) == 24);
    assert!(std::mem::offset_of!(GuardProfile, unique) == 40);
    assert!(std::mem::offset_of!(GuardProfile, image_size) == 56);
    assert!(size_of::<Platform>() == 72);
    assert!(std::mem::offset_of!(Platform, bones) == 64);
    assert!(size_of::<Stats>() == 40);
    assert!(size_of::<Coverage>() == 1472);
};
