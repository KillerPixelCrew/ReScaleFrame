// SPDX-License-Identifier: MIT
use super::*;
use rsf_game_sys::backend_result::NOT_COMPILED;

#[cfg(all(feature = "test-fixtures", not(test)))]
mod fixture;

#[cfg(not(test))]
fn platform() -> Option<&'static Platform> {
    super::sdk::platform()
}

#[cfg(test)]
fn platform() -> Option<&'static Platform> {
    None
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_dlss_available() -> u32 {
    u32::from(platform().is_some())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_load(setup: *const Setup) -> i32 {
    if let Some(p) = platform() {
        return unsafe { rsf_core_dlss_streamline_load(setup, p) };
    }
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*setup).abi_version } != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    NOT_COMPILED
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_set_device(device: Handle) -> i32 {
    if device.is_null() {
        return INVALID_ARGUMENT;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_set_device(device, p)
    })
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_native_present(
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    if chain.is_null() || begin > 1 {
        return INVALID_ARGUMENT;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_present(chain, interval, flags, begin, p)
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_query_support(support: *mut Support) -> i32 {
    if !unsafe { has_capacity(support) } {
        return INVALID_ARGUMENT;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_support(support, p)
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_plan_render_size(plan: *mut Plan) -> i32 {
    if !unsafe { has_capacity(plan) } {
        return INVALID_ARGUMENT;
    }
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_plan(plan, p)
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_evaluate(context: Handle, frame: *const Frame) -> i32 {
    if let Some(p) = platform() {
        return unsafe { rsf_core_dlss_streamline_evaluate(context, frame, 0, p) };
    }
    if context.is_null() || !unsafe { has_capacity(frame) } {
        return INVALID_ARGUMENT;
    }
    if unsafe { (*frame).abi_version } != RSF_DLSS_ABI_VERSION {
        return ABI_MISMATCH;
    }
    NOT_COMPILED
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_evaluate_shared(list: Handle, frame: *const Frame, id: u64) -> i32 {
    let Some(p) = platform() else {
        return NOT_COMPILED;
    };
    if id == 0 {
        return NOT_READY;
    }
    unsafe { rsf_core_dlss_streamline_evaluate(list, frame, id, p) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_share_host(host: Handle, log: Log, user: Handle) -> i32 {
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_share_host(host, log, user, p)
    })
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_release_viewport(viewport: u32) -> i32 {
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_release(viewport, p)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_dlss_release_resources() -> i32 {
    unsafe { rsf_dlss_release_viewport(0) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_dlss_shutdown() -> i32 {
    platform().map_or(NOT_COMPILED, |p| unsafe {
        rsf_core_dlss_streamline_shutdown(p)
    })
}
