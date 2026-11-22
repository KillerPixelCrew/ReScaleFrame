// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::same_device;
use crate::d3d11_output_abi::rsf_d3d11_output_is_immediate;
use crate::graphics_d3d11::rsf_call_native_log;
use crate::graphics_pass_abi::{
    RSF_FULLSCREEN_PASS_ABI_VERSION as ABI, RSF_PRESENT_BLIT_ABI_VERSION,
    rsf_fullscreen_draw as Draw, rsf_fullscreen_setup as Setup,
    rsf_present_blit_setup as PresentSetup,
};
use crate::sr_session::has_capacity;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, TryLockError};
use windows::core::Interface;

type Handle = *mut c_void;
#[path = "fullscreen_pass/d3d11.rs"]
mod d3d11;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const SHADER_FAILED: i32 = -3;
const RESOURCE_FAILED: i32 = -4;
const NO_BACK_BUFFER: i32 = -5;
const COPY: u32 = 0;
const TONEMAP: u32 = 1;
const PREMULTIPLIED: u32 = 2;
const PREMULTIPLIED_SRGB: u32 = 4;
const PREMULTIPLIED_GAMMA22: u32 = 5;
const COPY_RGBA: u32 = 6;
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
    assert!(size_of::<Setup>() == size_of::<PresentSetup>() && ABI == RSF_PRESENT_BLIT_ABI_VERSION);
    assert!(size_of::<Constants>() == 16);
};

struct Pass {
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

unsafe fn create(device: Handle, setup: *const Setup, out: *mut *mut Pass, filter: u32) -> i32 {
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
        filter: AtomicU32::new(filter),
        drawing: Mutex::new(()),
    });
    if pass.is_null() {
        return RESOURCE_FAILED;
    }
    if setup.log.is_some() {
        let say =
            |text: &CStr| unsafe { rsf_call_native_log(setup.log, setup.log_user, text.as_ptr()) };
        say(c"fullscreen pass ready");
        if filter == LINEAR {
            say(c"present blit ready");
        }
    }
    unsafe { out.write(pass) };
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fullscreen_pass_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Pass,
) -> i32 {
    unsafe { create(device, setup, out, POINT) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_present_blit_create(
    device: Handle,
    setup: *const PresentSetup,
    out: *mut *mut Pass,
) -> i32 {
    unsafe { create(device, setup.cast::<Setup>(), out, LINEAR) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fullscreen_pass_draw(
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
unsafe extern "C" fn rsf_present_blit_draw(
    pass: *const Pass,
    context: Handle,
    swapchain: Handle,
    source: Handle,
    tonemap: u32,
) -> i32 {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return INVALID;
    };
    if context.is_null() || swapchain.is_null() || source.is_null() {
        return INVALID;
    }
    let Some((back_buffer, size)) = (unsafe { d3d11::back_buffer(swapchain) }) else {
        return NO_BACK_BUFFER;
    };
    let Some((target, view)) =
        (unsafe { d3d11::present_views(pass.device.as_ptr(), &back_buffer, source) })
    else {
        return RESOURCE_FAILED;
    };
    let status = pass.draw(
        context,
        target.as_raw(),
        view.as_ptr(),
        &Draw {
            struct_size: size_of::<Draw>() as u32,
            mode: if tonemap != 0 { TONEMAP } else { COPY },
            exposure: 1.0,
            width: size[0],
            height: size[1],
        },
    );
    match status {
        OK | INVALID | ABI_MISMATCH => status,
        _ => RESOURCE_FAILED,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fullscreen_pass_set_filter(pass: *const Pass, filter: u32) {
    if let Some(pass) = unsafe { pass.as_ref() } {
        pass.filter.store(filter, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fullscreen_pass_destroy(pass: *mut Pass) {
    if !pass.is_null() {
        drop(unsafe { Box::from_raw(pass) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_present_blit_destroy(pass: *mut Pass) {
    unsafe { rsf_fullscreen_pass_destroy(pass) };
}
