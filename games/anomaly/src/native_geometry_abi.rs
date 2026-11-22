// SPDX-License-Identifier: MIT
//! Private layouts mirrored in native_geometry_platform.h.

use crate::engine_guards::Read;
use crate::geometry_registry::IdentityRegistry;
use crate::sys::{
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

/// The mapped executable and the index of its Rust-owned profiles
/// (`geometry_profile.rs`, `native_profile.rs`). Native code treats both as opaque.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Engine {
    pub module: usize,
    pub build: usize,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct ChildRange {
    pub first: usize,
    pub end: usize,
    pub capacity: usize,
}

/// The selected build's engine globals, resolved in Rust from the profile.
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

// The native writer owns the capture and the flush owns the draw facts. Both
// records live on the native stack through the synchronous callback.
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

/// The native hook set and the two guarded engine-memory leaves. Profiles,
/// layouts and site tables stay in Rust.
#[repr(C)]
pub(crate) struct Platform {
    pub created: unsafe extern "C" fn() -> bool,
    pub install: unsafe extern "C" fn(Engine, u32) -> bool,
    pub enable: unsafe extern "C" fn() -> bool,
    pub disable: unsafe extern "C" fn() -> bool,
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
    assert!(size_of::<Context>() == 16);
    assert!(size_of::<Window>() == 32);
    assert!(size_of::<Visual>() == 72);
    assert!(size_of::<Facts>() == 32);
    assert!(size_of::<Writer>() == 16);
    assert!(size_of::<Platform>() == 48);
    assert!(std::mem::offset_of!(Platform, bones) == 40);
    assert!(size_of::<Stats>() == 40);
    assert!(size_of::<Coverage>() == 1472);
};
