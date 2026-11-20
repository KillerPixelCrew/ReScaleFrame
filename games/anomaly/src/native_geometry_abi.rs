// SPDX-License-Identifier: MIT
//! Private layouts mirrored in native_geometry_platform.h.

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
#[derive(Default)]
pub(crate) struct ViewSample {
    pub frame: u32,
    pub active: u8,
    pub delay: u8,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct Palette {
    pub instances: usize,
    pub layout: u64,
    pub count: u32,
    pub mode: u16,
}

pub(crate) type Matrix = [f64; 16];

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

type Function = unsafe extern "C" fn();
type Invoke = unsafe extern "C" fn(Function, usize, *const c_void);

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Callback {
    pub function: Option<Function>,
    pub user: usize,
    pub invoke: Option<Invoke>,
}

#[repr(C)]
pub(crate) struct Writer {
    pub begin: unsafe extern "C" fn(*mut c_void, *const Visual, *const Matrix, u32) -> bool,
    pub bone: unsafe extern "C" fn(*mut c_void, u32, *const Matrix),
    pub finish: unsafe extern "C" fn(*mut c_void, u64) -> *const c_void,
}

#[repr(C)]
pub(crate) struct Platform {
    pub guard: unsafe extern "C" fn(Engine, u32) -> bool,
    pub created: unsafe extern "C" fn() -> bool,
    pub install: unsafe extern "C" fn(Engine, u32) -> bool,
    pub enable: unsafe extern "C" fn() -> bool,
    pub disable: unsafe extern "C" fn() -> bool,
    pub children: unsafe extern "C" fn(Engine, usize, *mut ChildRange) -> bool,
    pub pointer: unsafe extern "C" fn(usize, *mut usize) -> bool,
    pub pass: unsafe extern "C" fn(Engine) -> u8,
    pub view: unsafe extern "C" fn(Engine, *mut ViewSample) -> bool,
    pub world: unsafe extern "C" fn(Engine) -> usize,
    pub matrix: unsafe extern "C" fn(usize, *mut [f32; 16]) -> bool,
    pub skin_mode: unsafe extern "C" fn(usize, *mut u16) -> bool,
    pub palette: unsafe extern "C" fn(usize, *mut Palette) -> bool,
    pub bone_instances: unsafe extern "C" fn(usize) -> usize,
    pub bone: unsafe extern "C" fn(usize, u32, *mut [f32; 16]) -> bool,
    pub tree_lod: unsafe extern "C" fn(usize) -> i64,
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
    assert!(size_of::<ViewSample>() == 8);
    assert!(size_of::<Palette>() == 24);
    assert!(size_of::<Context>() == 16);
    assert!(size_of::<Window>() == 32);
    assert!(size_of::<Visual>() == 80);
    assert!(size_of::<Facts>() == 32);
    assert!(size_of::<Callback>() == 24);
    assert!(size_of::<Writer>() == 24);
    assert!(size_of::<Platform>() == 128);
    assert!(size_of::<Stats>() == 40);
    assert!(size_of::<Coverage>() == 1472);
};
