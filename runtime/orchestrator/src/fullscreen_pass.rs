// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::same_device;
use crate::graphics_d3d11::rsf_call_native_log;
use crate::sr_session::has_capacity;
use crate::sys::rsf_d3d11_output_is_immediate;
use crate::sys::{
    RSF_FULLSCREEN_PASS_ABI_VERSION as ABI, rsf_fullscreen_draw as Draw,
    rsf_fullscreen_setup as Setup,
};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, TryLockError};

type Handle = *mut c_void;
#[path = "fullscreen_pass/d3d11.rs"]
mod d3d11;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const RESOURCE_FAILED: i32 = -4;
pub(super) const COPY: u32 = 0;
const PREMULTIPLIED: u32 = 2;
const PREMULTIPLIED_SRGB: u32 = 4;
const PREMULTIPLIED_GAMMA22: u32 = 5;
pub(super) const COPY_RGBA: u32 = 6;
const POINT: u32 = 0;
const LINEAR: u32 = 1;

#[repr(C)]
struct Constants {
    mode: u32,
    exposure: f32,
    padding: [u32; 2],
}

const _: () = {
    assert!(size_of::<Setup>() == 24 && size_of::<Draw>() == 20);
    assert!(size_of::<Constants>() == 16);
};

pub(super) struct Pass {
    resources: [ComRef; 9],
    device: ComRef,
    filter: AtomicU32,
    drawing: Mutex<()>,
}

impl Pass {
    fn draw(&self, context: Handle, target: Handle, source: Handle, parameters: &Draw) -> i32 {
        let device = self.device.as_ptr();
        if parameters.mode > COPY_RGBA
            || unsafe { rsf_d3d11_output_is_immediate(context) } == 0
            || [context, target, source]
                .into_iter()
                .any(|child| !unsafe { same_device(device, child) })
        {
            return INVALID;
        }
        let size = if parameters.width != 0 && parameters.height != 0 {
            [parameters.width, parameters.height]
        } else {
            let Some(size) = (unsafe { d3d11::target_extent(target) }) else {
                return INVALID;
            };
            size
        };
        let _drawing = match self.drawing.try_lock() {
            Ok(drawing) => drawing,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return RESOURCE_FAILED,
        };
        let mode = parameters.mode;
        let constants = Constants {
            mode,
            exposure: if parameters.exposure > 0.0 {
                parameters.exposure
            } else {
                1.0
            },
            padding: [0; 2],
        };
        let linear = self.filter.load(Ordering::Relaxed) == LINEAR;
        let blend = matches!(
            mode,
            PREMULTIPLIED | PREMULTIPLIED_SRGB | PREMULTIPLIED_GAMMA22
        );
        if unsafe {
            d3d11::draw(
                context,
                target,
                source,
                &self.resources,
                size,
                linear,
                blend,
                &constants,
            )
        }
        .is_some()
        {
            OK
        } else {
            RESOURCE_FAILED
        }
    }
}

unsafe fn create(device: Handle, setup: *const Setup, out: *mut *mut Pass) -> i32 {
    if device.is_null() || out.is_null() || !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if setup.abi_version != ABI {
        return ABI_MISMATCH;
    }
    unsafe { out.write(null_mut()) };
    let resources = match unsafe { d3d11::make(device, setup) } {
        Ok(resources) => resources,
        Err(status) => return status,
    };
    let pass = crate::allocate_handle(Pass {
        resources,
        device: unsafe { ComRef::retain(device) },
        filter: AtomicU32::new(POINT),
        drawing: Mutex::new(()),
    });
    if pass.is_null() {
        return RESOURCE_FAILED;
    }
    if setup.log.is_some() {
        unsafe {
            rsf_call_native_log(setup.log, setup.log_user, c"fullscreen pass ready".as_ptr())
        };
    }
    unsafe { out.write(pass) };
    OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fullscreen_pass_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
) -> i32 {
    unsafe { create(device, setup, out) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fullscreen_pass_draw(
    pass: *const Pass,
    context: Handle,
    target: Handle,
    source: Handle,
    parameters: *const Draw,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null()
        || target.is_null()
        || source.is_null()
        || !unsafe { has_capacity(parameters) }
    {
        return INVALID;
    }
    pass.draw(context, target, source, unsafe { &*parameters })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fullscreen_pass_set_filter(pass: *const Pass, filter: u32) {
    if let Some(pass) = unsafe { pass.as_ref() } {
        pass.filter.store(filter, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fullscreen_pass_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}
