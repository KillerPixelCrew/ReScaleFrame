// SPDX-License-Identifier: MIT
use super::*;
use crate::sr_session::has_capacity;
use crate::streamline_sr::preset_valid;

const NOT_COMPILED: i32 = -3;
const ULTRA_PERFORMANCE: u32 = 4;

#[cfg(not(test))]
unsafe extern "C" {
    fn rsf_dlss_native12_sdk_platform() -> *const Platform;
}

#[cfg(not(test))]
fn platform() -> Option<&'static Platform> {
    unsafe { rsf_dlss_native12_sdk_platform().as_ref() }
}

#[cfg(test)]
fn platform() -> Option<&'static Platform> {
    None
}

unsafe fn validate_setup(device: Handle, setup: *const Setup) -> Result<(), i32> {
    if device.is_null() || !unsafe { has_capacity(setup) } {
        return Err(INVALID);
    }
    let setup = unsafe { &*setup };
    if setup.interposer_path_utf8.is_null()
        || setup.plugin_directory_utf8.is_null()
        || setup.engine_version_utf8.is_null()
        || (setup.application_id == 0 && setup.project_id_utf8.is_null())
        || !preset_valid(setup.preset)
    {
        return Err(INVALID);
    }
    if setup.abi_version != RSF_DLSS_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    Ok(())
}

unsafe fn register(
    device: Handle,
    setup: *const Setup,
    module: Handle,
    out: *mut *mut Registration,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return INVALID;
    };
    *out = null_mut();
    if let Err(result) = unsafe { validate_setup(device, setup) } {
        return result;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_native12_register(device, &*setup, module, out, p)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_registration_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Registration,
) -> i32 {
    unsafe { register(device, setup, null_mut(), out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_registration_borrow(
    device: Handle,
    module: Handle,
    setup: *const Setup,
    out: *mut *mut Registration,
) -> i32 {
    if module.is_null() {
        return INVALID;
    }
    unsafe { register(device, setup, module, out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_create_registered(
    device: Handle,
    setup: *const Setup,
    registration: *mut Registration,
    out: *mut *mut Context,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return INVALID;
    };
    *out = null_mut();
    if !unsafe { has_capacity(setup) } || registration.is_null() || platform().is_none() {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    if !preset_valid(setup.preset) {
        return INVALID;
    }
    rsf_core_dlss_native12_create_registered(device, setup, unsafe { &*registration }, out)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_registration_present(
    registration: *mut Registration,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    if registration.is_null() || swapchain.is_null() || platform().is_none() {
        return INVALID;
    }
    unsafe {
        rsf_core_dlss_native12_registration_present(
            &*registration,
            swapchain,
            interval,
            flags,
            begin,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_registration_destroy(registration: *mut Registration) {
    if platform().is_some() {
        unsafe { rsf_core_dlss_native12_registration_destroy(registration) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_create(
    device: Handle,
    setup: *const Setup,
    out: *mut *mut Context,
) -> i32 {
    unsafe { rsf_dlss_native12_create_shared(device, setup, null_mut(), out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_create_shared(
    device: Handle,
    setup: *const Setup,
    host: Handle,
    out: *mut *mut Context,
) -> i32 {
    let Some(out) = (unsafe { out.as_mut() }) else {
        return INVALID;
    };
    *out = null_mut();
    if let Err(result) = unsafe { validate_setup(device, setup) } {
        return result;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_native12_create(device, &*setup, host, out, p)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_plan(
    context: *mut Context,
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    if context.is_null()
        || width == 0
        || height == 0
        || render_width.is_null()
        || render_height.is_null()
        || quality > ULTRA_PERFORMANCE
    {
        return INVALID;
    }
    platform().map_or(NOT_COMPILED, |_| unsafe {
        rsf_core_dlss_native12_plan(
            &*context,
            width,
            height,
            quality,
            render_width,
            render_height,
        )
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_evaluate(
    context: *mut Context,
    list: Handle,
    frame: *const Frame,
) -> i32 {
    if context.is_null() || list.is_null() {
        return INVALID;
    }
    platform().map_or(NOT_COMPILED, |_| unsafe {
        rsf_core_dlss_native12_evaluate(&*context, list, frame, null_mut())
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_evaluate_borrowed(
    context: *mut Context,
    list: Handle,
    frame: *const Frame,
    token: Handle,
) -> i32 {
    if context.is_null() || list.is_null() || token.is_null() || platform().is_none() {
        return INVALID;
    }
    unsafe { rsf_core_dlss_native12_evaluate(&*context, list, frame, token) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_destroy(context: *mut Context) {
    if platform().is_some() {
        unsafe { rsf_core_dlss_native12_destroy(context) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_set_sdr(context: *mut Context, sdr: u32) {
    if !context.is_null() && platform().is_some() {
        rsf_core_dlss_native12_set_sdr(unsafe { &*context }, sdr);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_native12_present(
    context: *mut Context,
    swapchain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    if context.is_null() || swapchain.is_null() {
        return INVALID;
    }
    platform().map_or(NOT_COMPILED, |_| unsafe {
        rsf_core_dlss_native12_present(&*context, swapchain, interval, flags, begin)
    })
}
