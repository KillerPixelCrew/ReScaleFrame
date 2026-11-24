// SPDX-License-Identifier: MIT
use super::*;

#[cfg(all(feature = "test-fixtures", not(test)))]
mod fixture;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_load(setup: *const Setup) -> i32 {
    unsafe { rsf_core_dlss_streamline_load(setup) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_set_device(device: Handle) -> i32 {
    if device.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { rsf_core_dlss_streamline_set_device(device) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_query_support(support: *mut Support) -> i32 {
    unsafe { rsf_core_dlss_streamline_support(support) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_evaluate(context: Handle, frame: *const Frame) -> i32 {
    unsafe { rsf_core_dlss_streamline_evaluate(context, frame, 0) }
}

pub(crate) unsafe fn rsf_dlss_evaluate_shared(list: Handle, frame: *const Frame, id: u64) -> i32 {
    if id == 0 {
        return NOT_READY;
    }
    unsafe { rsf_core_dlss_streamline_evaluate(list, frame, id) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_share_host(host: Handle, log: Log, user: Handle) -> i32 {
    unsafe { rsf_core_dlss_streamline_share_host(host, log, user) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_release_resources() -> i32 {
    unsafe { rsf_dlss_release_viewport(0) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_dlss_shutdown() -> i32 {
    unsafe { rsf_core_dlss_streamline_shutdown() }
}
