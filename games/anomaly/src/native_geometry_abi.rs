// SPDX-License-Identifier: MIT
//! The geometry owner's constants and records.

use crate::sys::*;
use std::ffi::c_void;

pub(crate) const UNSET: u8 = RSF_ANOMALY_GEOMETRY_UNSET as u8;
pub(crate) const RIGID: u8 = RSF_ANOMALY_GEOMETRY_RIGID as u8;
pub(crate) const HARDWARE_SKINNED: u8 = RSF_ANOMALY_GEOMETRY_HARDWARE_SKINNED as u8;
pub(crate) const SINGLE_BONE: u8 = RSF_ANOMALY_GEOMETRY_SINGLE_BONE as u8;
pub(crate) const SOFTWARE_SKINNED: u8 = RSF_ANOMALY_GEOMETRY_SOFTWARE_SKINNED as u8;
pub(crate) const TREE: u8 = RSF_ANOMALY_GEOMETRY_TREE as u8;
pub(crate) const MAIN: u8 = RSF_ANOMALY_GEOMETRY_MAIN as u8;
pub(crate) const SCOPE: u8 = RSF_ANOMALY_GEOMETRY_SCOPE as u8;
pub(crate) const WORLD: u8 = RSF_ANOMALY_GEOMETRY_WORLD as u8;
pub(crate) const FIRST_PERSON: u8 = RSF_ANOMALY_GEOMETRY_FIRST_PERSON as u8;

pub(crate) const PASSES: usize = RSF_ANOMALY_PASS_COUNT as usize;
pub(crate) const TYPES: usize = RSF_ANOMALY_TYPE_COUNT as usize;
pub(crate) const VERDICTS: usize = RSF_ANOMALY_DRAW_COUNT as usize;

// The records native_geometry_platform.h declares, bound in crate::sys. `Visual::type_` and `Facts::type_` are the
// visual type the header calls `type`.
pub(crate) use crate::sys::{
    rsf_anomaly_geometry_context as Context, rsf_anomaly_geometry_coverage as Coverage,
    rsf_anomaly_geometry_draw_facts as NativeDrawFacts, rsf_anomaly_geometry_engine as Engine,
    rsf_anomaly_geometry_facts as Facts, rsf_anomaly_geometry_native_matrix as NativeBones,
    rsf_anomaly_geometry_platform as Platform, rsf_anomaly_geometry_snapshot as NativeDrawCapture,
    rsf_anomaly_geometry_stats as Stats, rsf_anomaly_geometry_visual as Visual,
    rsf_anomaly_geometry_window as Window, rsf_anomaly_geometry_writer as Writer,
};

/// The engine's child vector (begin, end, capacity), copied through the guarded read.
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
pub(crate) type ReadBones = unsafe extern "C" fn(usize, usize, u32, *mut NativeBones) -> bool;

pub(crate) type Retired = unsafe extern "C" fn(*mut c_void, *const rsf_anomaly_geometry_key);
pub(crate) type DrawSink = unsafe extern "C" fn(*mut c_void, *const NativeDrawCapture);
pub(crate) type Flush = unsafe extern "C" fn(*const NativeDrawFacts);
pub(crate) type Near = unsafe extern "C" fn(*mut c_void);

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<ChildRange>() == 24);
