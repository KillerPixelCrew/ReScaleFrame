// SPDX-License-Identifier: MIT
//! Private call-scoped ABI with motion_stage_platform.h.
use super::*;
use crate::motion_device::{Platform as DevicePlatform, ReplayPlatform};
use crate::motion_device_abi::rsf_anomaly_motion_device_view as DeviceView;
use crate::motion_target::Platform as BindPlatform;
use crate::motion_target_abi::rsf_anomaly_motion_bind_layout as BindLayout;
use crate::shader_loader::{EngineIncludeOps, EngineShaderProfile, FrameLayout, Read};
use std::ffi::c_char;

pub(super) type Raw = *mut std::ffi::c_void;
pub(super) const PLATFORM_ABI: u32 = 12;

#[repr(C)]
pub(super) struct DrawScope([std::mem::MaybeUninit<u64>; 22]);

#[repr(C)]
pub(super) struct PositionViews {
    pub rtv: Raw,
    pub surface: Raw,
}

#[repr(C)]
pub(super) struct TargetFacts {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub layers: u32,
}

#[repr(C)]
pub(super) struct DrawFacts {
    pub capture: *const GeometrySnapshotFacts,
    pub visual: u64,
    pub tree_lod: i64,
    pub pass: u8,
    pub kind: u8,
    pub verdict: u8,
    pub view: u8,
    pub domain: u8,
    pub in_visual: bool,
    pub detail: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(in crate::motion) struct ShaderObjects {
    pub vs: Raw,
    pub hs: Raw,
    pub ds: Raw,
    pub ps: Raw,
    pub gs: Raw,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct DrawState {
    pub viewport: [f32; 4],
    pub jitter: [f32; 4],
}

#[repr(C)]
pub(super) struct ShaderCreation {
    pub target: *const c_char,
    pub bytecode: *const std::ffi::c_void,
    pub file: *const c_char,
    pub result: *const Raw,
    pub return_address: *const std::ffi::c_void,
    pub stack: *const std::ffi::c_void,
    pub status: i32,
    pub size: u32,
}

#[repr(C)]
pub(super) struct Platform {
    pub struct_size: u32,
    pub abi_version: u32,
    pub device: &'static DevicePlatform,
    pub bind: &'static BindPlatform,
    pub bind_layout: BindLayout,
    pub guards: unsafe extern "C" fn(Raw, u32, Raw) -> *const c_char,
    pub geometry_active: unsafe extern "C" fn() -> bool,
    pub observe_flush: unsafe extern "C" fn(bool),
    pub foliage_identity: unsafe extern "C" fn(bool),
    pub install_hooks: unsafe extern "C" fn(Raw, u32, Raw) -> bool,
    pub disable_hooks: unsafe extern "C" fn() -> bool,
    pub hooks_created: unsafe extern "C" fn() -> bool,
    pub install_draw_hooks: unsafe extern "C" fn(Raw) -> bool,
    pub render_target: unsafe extern "C" fn() -> Raw,
    pub position_views: unsafe extern "C" fn(Raw, *mut PositionViews) -> bool,
    pub native_frame: unsafe extern "C" fn(&mut u32) -> bool,
    pub render_view: unsafe extern "C" fn(&mut u8) -> bool,
    pub reset_generation: unsafe extern "C" fn() -> u64,
    pub options_word3: unsafe extern "C" fn(&mut u32) -> bool,
    pub target_facts: unsafe extern "C" fn(Raw, *mut TargetFacts) -> bool,
    pub backend: unsafe extern "C" fn() -> Raw,
    pub context: unsafe extern "C" fn() -> Raw,
    pub shader_device: unsafe extern "C" fn() -> Raw,
    pub clear_target: unsafe extern "C" fn(Raw, Raw),
    pub bound_shaders: unsafe extern "C" fn(Raw, &mut ShaderObjects, bool),
    pub set_shaders: unsafe extern "C" fn(Raw, &ShaderObjects, bool),
    pub replay: &'static ReplayPlatform,
    pub camera: unsafe extern "C" fn(&mut [f32; 16], &mut [f32; 16]) -> bool,
    pub world: unsafe extern "C" fn(&mut [f32; 16]) -> bool,
    pub read_constant: unsafe extern "C" fn(u32, u32, u32, Raw, u32) -> bool,
    pub write_constant: unsafe extern "C" fn(u32, u32, u32, *const std::ffi::c_void, u32) -> bool,
    pub draw_state: unsafe extern "C" fn(Raw, &mut DrawState),
    pub includes: &'static EngineIncludeOps,
    pub shader_profile: unsafe extern "C" fn(Raw, *mut EngineShaderProfile),
    pub frame_layout: unsafe extern "C" fn(*mut FrameLayout) -> Raw,
    pub read_engine: Read,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct ConstantRef {
    pub present: bool,
    pub buffer: u32,
    pub buffer_size: u32,
    pub offset: u32,
    pub size: u32,
}

#[repr(C)]
#[derive(Default)]
pub(crate) struct FoliageLayout {
    pub family: u8,
    pub xform: ConstantRef,
    pub consts: ConstantRef,
    pub wave: ConstantRef,
    pub wind: ConstantRef,
    pub array: ConstantRef,
    pub usable: bool,
}

pub(super) use crate::motion_frame_target_abi::rsf_anomaly_motion_frame_target as FrameTarget;

const _: () = {
    assert!(size_of::<PositionViews>() == 16);
    assert!(size_of::<DrawFacts>() == 32);
    assert!(size_of::<ShaderObjects>() == 40);
    assert!(size_of::<ShaderCreation>() == 56);
    assert!(std::mem::offset_of!(ShaderCreation, return_address) == 32);
    assert!(std::mem::offset_of!(ShaderCreation, status) == 48);
    assert!(size_of::<FoliageLayout>() == 108);
    assert!(std::mem::offset_of!(FoliageLayout, usable) == 104);
    assert!(size_of::<FrameTarget>() == 24);
    assert!(size_of::<Platform>() == 288);
    assert!(std::mem::offset_of!(Platform, guards) == 40);
    assert!(std::mem::offset_of!(Platform, render_target) == 104);
    assert!(std::mem::offset_of!(Platform, target_facts) == 152);
    assert!(std::mem::offset_of!(Platform, includes) == 256);
    assert!(std::mem::offset_of!(Platform, shader_profile) == 264);
    assert!(std::mem::offset_of!(Platform, frame_layout) == 272);
    assert!(std::mem::offset_of!(Platform, read_engine) == 280);
};
