// SPDX-License-Identifier: MIT
//! Private call-scoped ABI with motion_stage_platform.h.
use super::*;
use crate::motion_device_abi::{
    rsf_anomaly_motion_device_platform as DevicePlatform,
    rsf_anomaly_motion_device_view as DeviceView,
};
use crate::motion_target_abi::{
    rsf_anomaly_motion_bind_layout as BindLayout, rsf_anomaly_motion_bind_platform as BindPlatform,
};
use crate::shader_loader::{Bytes, Entry, FrameLayout, Manifest, Read, Request as ShaderRequest};
use std::ffi::c_char;

pub(super) type Raw = *mut std::ffi::c_void;
pub(super) type Redraw = unsafe extern "C" fn(Raw, Raw);
pub(super) const PLATFORM_ABI: u32 = 5;
type Retire = unsafe extern "C" fn(Raw, &InstanceKey);

#[repr(C)]
pub(super) struct WindowFacts {
    pub device: Raw,
    pub context: Raw,
    pub backend: Raw,
    pub position_rtv: Raw,
    pub position_surface: Raw,
    pub generation: u64,
    pub frame: u32,
    pub view: u8,
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
pub(super) struct ShaderResult {
    pub reflection: Raw,
    pub detail: Bytes,
    pub skin: i32,
    pub outcome: u8,
}

impl Default for ShaderResult {
    fn default() -> Self {
        Self {
            reflection: ptr::null_mut(),
            detail: (&[][..]).into(),
            skin: -2,
            outcome: 9,
        }
    }
}

#[repr(C)]
pub(super) struct PackInfo {
    // Owned by the native pack and immutable until its close leaf runs.
    pub manifest: *const Manifest,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct PatchCounters {
    pub intercepted: u64,
    pub built: u64,
    pub cache_hits: u64,
    pub refused: u64,
    pub refusals: [u64; 11],
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
    pub window_facts: unsafe extern "C" fn(Raw, *mut WindowFacts) -> bool,
    pub target_facts: unsafe extern "C" fn(Raw, *mut TargetFacts) -> i32,
    pub backend: unsafe extern "C" fn() -> Raw,
    pub context: unsafe extern "C" fn() -> Raw,
    pub shader_device: unsafe extern "C" fn() -> Raw,
    pub clear_target: unsafe extern "C" fn(Raw, Raw),
    pub bound_shaders: unsafe extern "C" fn(Raw, &mut ShaderObjects, bool),
    pub set_shaders: unsafe extern "C" fn(Raw, &ShaderObjects, bool),
    pub replay: unsafe extern "C" fn(&DeviceView, Raw, Redraw, Raw) -> u8,
    pub camera: unsafe extern "C" fn(&mut [f32; 16], &mut [f32; 16]) -> bool,
    pub world: unsafe extern "C" fn(&mut [f32; 16]) -> bool,
    pub drain: unsafe extern "C" fn(Retire, Raw) -> usize,
    pub read_constant: unsafe extern "C" fn(u32, u32, u32, Raw, u32) -> bool,
    pub write_constant: unsafe extern "C" fn(u32, u32, u32, *const std::ffi::c_void, u32) -> bool,
    pub draw_state: unsafe extern "C" fn(Raw, &mut DrawState),
    pub open_pack: unsafe extern "C" fn(
        Raw,
        Raw,
        *const u16,
        *const u16,
        *mut PackInfo,
        *mut c_char,
        usize,
    ) -> Raw,
    pub close_pack: unsafe extern "C" fn(Raw),
    pub activate_pack: unsafe extern "C" fn(Raw),
    pub attach_device: unsafe extern "C" fn(Raw, Raw),
    pub dump_shaders: unsafe extern "C" fn(Raw, *const u16) -> i32,
    pub patch_counters: unsafe extern "C" fn(Raw, &mut PatchCounters),
    pub frame_layout: unsafe extern "C" fn(*mut FrameLayout) -> Raw,
    pub read_engine: Read,
    pub prepare_shader: unsafe extern "C" fn(Raw, &ShaderRequest, &Entry, &mut ShaderResult),
    pub drop_reflection: unsafe extern "C" fn(Raw),
    pub foliage_layout: unsafe extern "C" fn(Raw, u8, &mut FoliageLayout) -> bool,
    pub link_shaders: unsafe extern "C" fn(Raw, Raw, Raw, Raw) -> bool,
    pub create_shader: unsafe extern "C" fn(Raw, *const c_char, Raw, Raw, bool, &mut bool) -> Raw,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ConstantRef {
    pub present: bool,
    pub buffer: u32,
    pub buffer_size: u32,
    pub offset: u32,
    pub size: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct FoliageLayout {
    family: u8,
    pub xform: ConstantRef,
    pub consts: ConstantRef,
    pub wave: ConstantRef,
    pub wind: ConstantRef,
    pub array: ConstantRef,
    pub usable: bool,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct FrameTarget {
    pub texture: Raw,
    pub width: u32,
    pub height: u32,
    pub frame: u32,
    pub main: bool,
    pub written: bool,
}

const _: () = {
    assert!(size_of::<WindowFacts>() == 56);
    assert!(size_of::<DrawFacts>() == 32);
    assert!(size_of::<ShaderObjects>() == 40);
    assert!(size_of::<ShaderResult>() == 32);
    assert!(size_of::<PackInfo>() == 8);
    assert!(size_of::<FoliageLayout>() == 108);
    assert!(std::mem::offset_of!(FoliageLayout, usable) == 104);
    assert!(size_of::<FrameTarget>() == 24);
    assert!(size_of::<Platform>() == 328);
    assert!(std::mem::offset_of!(Platform, guards) == 40);
    assert!(std::mem::offset_of!(Platform, frame_layout) == 272);
    assert!(std::mem::offset_of!(Platform, create_shader) == 320);
};
