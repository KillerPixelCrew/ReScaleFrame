// SPDX-License-Identifier: MIT
//! Windows loader bindings, the bridge exports and the lifecycle entry points
//! the native Vulkan services read.

mod bridge;
mod wsi;

#[cfg(not(test))]
pub(crate) use bridge::lifecycle_leaf;
pub(crate) use bridge::{fg_apply, fg_options_bound, fg_release};

use crate::lifecycle::Lifecycle;
use std::ffi::{CStr, c_char, c_void};
use std::io::{self, Write};

static LIFECYCLE: Lifecycle = Lifecycle::new();

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_take_present_error() -> i32 {
    LIFECYCLE.diagnostics.take_present_error()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_worker_timeout_epoch() -> u64 {
    LIFECYCLE.diagnostics.worker_timeout_epoch()
}

/// Streamline's log callback (`sl::LogType`, message).
#[cfg_attr(test, allow(dead_code))]
unsafe extern "C" fn rsf_slvk_log_message(kind: u32, message: *const c_char) {
    if message.is_null() {
        return;
    }
    let message = unsafe { CStr::from_ptr(message) };
    if !LIFECYCLE.diagnostics.message(message.to_bytes(), kind == 0) {
        return;
    }
    let level = match kind {
        2 => "error",
        1 => "warning",
        _ => "info",
    };
    let mut stderr = io::stderr().lock();
    let _ = write!(stderr, "[Streamline {level}] ");
    let _ = stderr.write_all(message.to_bytes());
    let _ = stderr.write_all(b"\n");
}

/// The interposer while the SDK is initialized, otherwise null.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_module() -> *mut c_void {
    LIFECYCLE.module()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_current_frame_token() -> usize {
    LIFECYCLE.frame_token.current()
}
