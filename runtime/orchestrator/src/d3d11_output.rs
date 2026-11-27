// SPDX-License-Identifier: MIT
use crate::fullscreen_pass::{
    COPY, COPY_RGBA, Pass, rsf_fullscreen_pass_create, rsf_fullscreen_pass_destroy,
    rsf_fullscreen_pass_draw,
};
use crate::sys::{RSF_FULLSCREEN_PASS_ABI_VERSION, rsf_fullscreen_draw, rsf_fullscreen_setup};
use std::ffi::c_void;
use std::ptr;
use std::ptr::NonNull;
use windows::core::{IUnknown, Interface};

// A fullscreen copy pass on one device, in plain or RGBA copy mode.
pub(super) struct FullscreenBlit {
    pass: NonNull<Pass>,
    mode: u32,
}

impl FullscreenBlit {
    pub(super) unsafe fn new_copy(device: &IUnknown, rgba: bool) -> Option<Self> {
        let setup = rsf_fullscreen_setup {
            struct_size: size_of::<rsf_fullscreen_setup>() as u32,
            abi_version: RSF_FULLSCREEN_PASS_ABI_VERSION,
            log: None,
            log_user: ptr::null_mut(),
        };
        let mut pass = ptr::null_mut();
        if unsafe { rsf_fullscreen_pass_create(device.as_raw(), &setup, &mut pass) } != 0 {
            return None;
        }
        Some(Self {
            pass: NonNull::new(pass)?,
            mode: if rgba { COPY_RGBA } else { COPY },
        })
    }

    pub(super) unsafe fn draw(
        &self,
        context: *mut c_void,
        target: *mut c_void,
        source: *mut c_void,
        width: u32,
        height: u32,
    ) -> bool {
        let draw = rsf_fullscreen_draw {
            struct_size: size_of::<rsf_fullscreen_draw>() as u32,
            mode: self.mode,
            exposure: 1.0,
            width,
            height,
        };
        unsafe { rsf_fullscreen_pass_draw(self.pass.as_ptr(), context, target, source, &draw) == 0 }
    }
}

impl Drop for FullscreenBlit {
    fn drop(&mut self) {
        unsafe { rsf_fullscreen_pass_destroy(self.pass.as_ptr()) };
    }
}

// The shared D3D11 helpers live in rsf-graphics with the adapter-facing leaves.
pub(super) use rsf_graphics::d3d11_output::{
    GraphicsCache, SavedBindings, same_device, texture_desc,
};
