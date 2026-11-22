// SPDX-License-Identifier: MIT
//! Direct `slSetConstants` call with Rust mirrors of the Streamline 2.14.1 records.
//!
//! `sl.h` is a C++ header, but the interposer exports `slSetConstants` with C linkage and the
//! references in its signature are pointers. The mirrors repeat the SDK's versioned header
//! (`next`, GUID, `size_t` version) and member order. The layout assertions use the sizes and
//! offsets that MSVC reports for `sl::Constants` and `sl::ViewportHandle` in the SDK headers.
use super::{Result, check};
use crate::sys::{rsf_streamline_host, rsf_streamline_host_module};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::c_void;
use std::mem::{offset_of, size_of, transmute};
use std::ptr;
use windows::{
    Win32::{Foundation::HMODULE, System::LibraryLoader::GetProcAddress},
    core::s,
};

#[repr(C)]
struct StructType {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const INVALID_FLOAT: f32 = f32::MAX;
const FALSE: u8 = 0;
const TRUE: u8 = 1;
const INVALID: u8 = 2;
const STRUCT_VERSION_1: usize = 1;
const STRUCT_VERSION_2: usize = 2;
const OK: i32 = 0;
const CONSTANTS_TYPE: StructType = StructType {
    data1: 0xdcd3_5ad7,
    data2: 0x4e4a,
    data3: 0x4bad,
    data4: [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
};
const VIEWPORT_TYPE: StructType = StructType {
    data1: 0x171b_6435,
    data2: 0x9b3c,
    data3: 0x4fc8,
    data4: [0x99, 0x94, 0xfb, 0xe5, 0x25, 0x69, 0xaa, 0xa4],
};

/// `sl::Constants`, structure version 2.
#[repr(C)]
struct Constants {
    next: *mut c_void,
    struct_type: StructType,
    struct_version: usize,
    camera_view_to_clip: [f32; 16],
    clip_to_camera_view: [f32; 16],
    clip_to_lens_clip: [f32; 16],
    clip_to_prev_clip: [f32; 16],
    prev_clip_to_clip: [f32; 16],
    jitter_offset: [f32; 2],
    mvec_scale: [f32; 2],
    camera_pinhole_offset: [f32; 2],
    camera_pos: [f32; 3],
    camera_up: [f32; 3],
    camera_right: [f32; 3],
    camera_fwd: [f32; 3],
    camera_near: f32,
    camera_far: f32,
    camera_fov: f32,
    camera_aspect_ratio: f32,
    motion_vectors_invalid_value: f32,
    depth_inverted: u8,
    camera_motion_included: u8,
    motion_vectors_3d: u8,
    reset: u8,
    orthographic_projection: u8,
    motion_vectors_dilated: u8,
    motion_vectors_jittered: u8,
    min_relative_linear_depth_object_separation: f32,
}

/// `sl::ViewportHandle`, structure version 1.
#[repr(C)]
struct ViewportHandle {
    next: *mut c_void,
    struct_type: StructType,
    struct_version: usize,
    value: u32,
}

const _: () = {
    assert!(size_of::<Constants>() == 456);
    assert!(offset_of!(Constants, struct_type) == 8);
    assert!(offset_of!(Constants, struct_version) == 24);
    assert!(offset_of!(Constants, camera_view_to_clip) == 32);
    assert!(offset_of!(Constants, clip_to_camera_view) == 96);
    assert!(offset_of!(Constants, clip_to_lens_clip) == 160);
    assert!(offset_of!(Constants, clip_to_prev_clip) == 224);
    assert!(offset_of!(Constants, prev_clip_to_clip) == 288);
    assert!(offset_of!(Constants, jitter_offset) == 352);
    assert!(offset_of!(Constants, mvec_scale) == 360);
    assert!(offset_of!(Constants, camera_pinhole_offset) == 368);
    assert!(offset_of!(Constants, camera_pos) == 376);
    assert!(offset_of!(Constants, camera_up) == 388);
    assert!(offset_of!(Constants, camera_right) == 400);
    assert!(offset_of!(Constants, camera_fwd) == 412);
    assert!(offset_of!(Constants, camera_near) == 424);
    assert!(offset_of!(Constants, camera_far) == 428);
    assert!(offset_of!(Constants, camera_fov) == 432);
    assert!(offset_of!(Constants, camera_aspect_ratio) == 436);
    assert!(offset_of!(Constants, motion_vectors_invalid_value) == 440);
    assert!(offset_of!(Constants, depth_inverted) == 444);
    assert!(offset_of!(Constants, reset) == 447);
    assert!(offset_of!(Constants, orthographic_projection) == 448);
    assert!(offset_of!(Constants, motion_vectors_jittered) == 450);
    assert!(offset_of!(Constants, min_relative_linear_depth_object_separation) == 452);
    assert!(size_of::<ViewportHandle>() == 40);
    assert!(offset_of!(ViewportHandle, value) == 32);
};

impl Constants {
    /// The member defaults that the SDK constructors apply.
    fn new() -> Self {
        Self {
            next: ptr::null_mut(),
            struct_type: CONSTANTS_TYPE,
            struct_version: STRUCT_VERSION_2,
            camera_view_to_clip: [INVALID_FLOAT; 16],
            clip_to_camera_view: [INVALID_FLOAT; 16],
            clip_to_lens_clip: [INVALID_FLOAT; 16],
            clip_to_prev_clip: [INVALID_FLOAT; 16],
            prev_clip_to_clip: [INVALID_FLOAT; 16],
            jitter_offset: [INVALID_FLOAT; 2],
            mvec_scale: [INVALID_FLOAT; 2],
            camera_pinhole_offset: [INVALID_FLOAT; 2],
            camera_pos: [INVALID_FLOAT; 3],
            camera_up: [INVALID_FLOAT; 3],
            camera_right: [INVALID_FLOAT; 3],
            camera_fwd: [INVALID_FLOAT; 3],
            camera_near: INVALID_FLOAT,
            camera_far: INVALID_FLOAT,
            camera_fov: INVALID_FLOAT,
            camera_aspect_ratio: INVALID_FLOAT,
            motion_vectors_invalid_value: INVALID_FLOAT,
            depth_inverted: INVALID,
            camera_motion_included: INVALID,
            motion_vectors_3d: INVALID,
            reset: INVALID,
            orthographic_projection: FALSE,
            motion_vectors_dilated: FALSE,
            motion_vectors_jittered: FALSE,
            min_relative_linear_depth_object_separation: 40.0,
        }
    }

    /// The FG-style constants the fixture sets for one frame, from the shared render pass.
    fn from_pass(pass: &rsf_game_render_pass) -> Self {
        let camera = &pass.camera;
        Self {
            camera_view_to_clip: camera.view_to_clip,
            clip_to_camera_view: camera.clip_to_view,
            clip_to_prev_clip: camera.clip_to_previous_clip,
            prev_clip_to_clip: pass.previous_clip_to_clip,
            camera_near: 0.2,
            camera_far: 1000.0,
            camera_fov: camera.vertical_fov_radians,
            camera_aspect_ratio: 640.0 / 360.0,
            depth_inverted: TRUE,
            reset: TRUE,
            ..Self::new()
        }
    }
}

type SetConstants =
    unsafe extern "C" fn(*const Constants, *const c_void, *const ViewportHandle) -> i32;

/// Sets constants on viewport zero with the host's real frame token.
pub(super) fn set_shared_constants(
    host: *mut rsf_streamline_host,
    token: *const c_void,
    pass: &rsf_game_render_pass,
) -> Result<()> {
    check(!token.is_null(), "Streamline host returned no frame token")?;
    let module = unsafe { rsf_streamline_host_module(host) };
    check(!module.is_null(), "Streamline host returned no module")?;
    // The export is `sl::Result slSetConstants(const Constants&, const FrameToken&,
    // const ViewportHandle&)`, which the C ABI passes as three pointers.
    let export = unsafe { GetProcAddress(HMODULE(module), s!("slSetConstants")) };
    let export = export.ok_or("Streamline module does not export slSetConstants")?;
    let set_constants: SetConstants = unsafe { transmute(export) };
    let constants = Constants::from_pass(pass);
    let viewport = ViewportHandle {
        next: ptr::null_mut(),
        struct_type: VIEWPORT_TYPE,
        struct_version: STRUCT_VERSION_1,
        value: 0,
    };
    check(
        unsafe { set_constants(&constants, token, &viewport) } == OK,
        "Streamline refused independent FG constants",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_carry_the_sdk_header_and_defaults() {
        let mut pass: rsf_game_render_pass = unsafe { std::mem::zeroed() };
        pass.camera.vertical_fov_radians = 1.0;
        pass.camera.view_to_clip[5] = 3.0;
        let constants = Constants::from_pass(&pass);
        let bytes = unsafe {
            std::slice::from_raw_parts((&raw const constants).cast::<u8>(), size_of::<Constants>())
        };
        assert_eq!(&bytes[8..12], &0xdcd3_5ad7u32.to_le_bytes());
        assert_eq!(&bytes[24..32], &2usize.to_le_bytes());
        assert_eq!(&bytes[32 + 5 * 4..32 + 6 * 4], &3.0f32.to_le_bytes());
        assert_eq!(&bytes[160..164], &f32::MAX.to_le_bytes());
        assert_eq!(&bytes[444..448], &[TRUE, INVALID, INVALID, TRUE]);
        assert_eq!(&bytes[452..456], &40.0f32.to_le_bytes());
    }
}
