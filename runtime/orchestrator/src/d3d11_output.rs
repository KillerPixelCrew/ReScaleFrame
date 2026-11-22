// SPDX-License-Identifier: MIT
use crate::fullscreen_pass::{
    COPY, COPY_RGBA, rsf_fullscreen_pass_create, rsf_fullscreen_pass_destroy,
    rsf_fullscreen_pass_draw,
};
use crate::sys::rsf_fullscreen_pass;
use crate::sys::{RSF_FULLSCREEN_PASS_ABI_VERSION, rsf_fullscreen_draw, rsf_fullscreen_setup};
use std::ffi::c_void;
use std::ptr;
use std::ptr::NonNull;
use windows::core::{IUnknown, Interface};

pub(super) type CreateBlit = unsafe extern "C" fn(*mut c_void) -> *mut rsf_fullscreen_pass;
pub(super) type DrawBlit = unsafe extern "C" fn(
    *mut rsf_fullscreen_pass,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    u32,
    u32,
) -> u32;
pub(super) type DestroyBlit = unsafe extern "C" fn(*mut rsf_fullscreen_pass);

pub(super) struct FullscreenBlit {
    pass: NonNull<rsf_fullscreen_pass>,
    draw: DrawBlit,
    destroy: DestroyBlit,
}

unsafe extern "C" fn create_copy(device: *mut c_void) -> *mut rsf_fullscreen_pass {
    let setup = rsf_fullscreen_setup {
        struct_size: size_of::<rsf_fullscreen_setup>() as u32,
        abi_version: RSF_FULLSCREEN_PASS_ABI_VERSION,
        log: None,
        log_user: ptr::null_mut(),
    };
    let mut pass = ptr::null_mut();
    if unsafe { rsf_fullscreen_pass_create(device, &setup, &mut pass) } == 0 {
        pass.cast()
    } else {
        ptr::null_mut()
    }
}

unsafe extern "C" fn draw_copy<const MODE: u32>(
    pass: *mut rsf_fullscreen_pass,
    context: *mut c_void,
    target: *mut c_void,
    source: *mut c_void,
    width: u32,
    height: u32,
) -> u32 {
    let draw = rsf_fullscreen_draw {
        struct_size: size_of::<rsf_fullscreen_draw>() as u32,
        mode: MODE,
        exposure: 1.0,
        width,
        height,
    };
    u32::from(unsafe { rsf_fullscreen_pass_draw(pass.cast(), context, target, source, &draw) } == 0)
}

unsafe extern "C" fn destroy_copy(pass: *mut rsf_fullscreen_pass) {
    unsafe { rsf_fullscreen_pass_destroy(pass.cast()) };
}

impl FullscreenBlit {
    pub(super) unsafe fn new_copy(device: &IUnknown, rgba: bool) -> Option<Self> {
        let draw = if rgba {
            draw_copy::<COPY_RGBA>
        } else {
            draw_copy::<COPY>
        };
        unsafe { Self::new(device, create_copy, draw, destroy_copy) }
    }

    pub(super) unsafe fn new(
        device: &IUnknown,
        create: CreateBlit,
        draw: DrawBlit,
        destroy: DestroyBlit,
    ) -> Option<Self> {
        let pass = NonNull::new(unsafe { create(device.as_raw()) })?;
        Some(Self {
            pass,
            draw,
            destroy,
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
        unsafe { (self.draw)(self.pass.as_ptr(), context, target, source, width, height) != 0 }
    }
}

impl Drop for FullscreenBlit {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.pass.as_ptr()) };
    }
}

// The shared D3D11 helpers live in rsf-graphics with the adapter-facing leaves.
pub(super) use rsf_graphics::d3d11_output::{
    GraphicsCache, SavedBindings, same_device, texture_desc,
};
