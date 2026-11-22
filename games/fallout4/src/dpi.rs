// SPDX-License-Identifier: MIT
//! Per-monitor v2 DPI awareness before the engine creates its window or device.
//!
//! The request (`rsf_game_sys::dpi`) is safe only while the renderer owner has neither; this
//! adapter refuses prepare when it is not verified.
use crate::build_layout::{
    RENDERER_DEVICE as DEVICE, RENDERER_POINTER as OWNER_RVA, RENDERER_WINDOW as WINDOW,
};
use crate::{Threw, image, message, read};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::CStr;
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};

/// The renderer owner's device and window, or None when the owner is unreadable.
fn owner() -> Option<(usize, usize)> {
    match read::<usize>(image() + OWNER_RVA)? {
        0 => Some((0, 0)),
        renderer => Some((
            read::<usize>(renderer.wrapping_add(DEVICE))?,
            read::<usize>(renderer.wrapping_add(WINDOW))?,
        )),
    }
}

/// Requests and verifies startup awareness. Returns the refusal reason, if any. The thread's
/// last error is preserved.
pub(crate) fn prepare(host: &rsf_game_host_services) -> Result<Option<&'static CStr>, Threw> {
    let saved_error = unsafe { GetLastError() };
    let result = request(host);
    unsafe { SetLastError(saved_error) };
    result
}

fn request(host: &rsf_game_host_services) -> Result<Option<&'static CStr>, Threw> {
    let owner = owner();
    let owner_read = owner.is_some();
    let (device, window) = owner.unwrap_or((0, 0));
    let cold = owner_read && device == 0 && window == 0;
    let startup = rsf_game_sys::dpi::request_per_monitor_v2(cold);
    if host.log.is_some() {
        let flag = |value: bool| u32::from(value);
        let text = message(format!(
            "Fallout4 DPI startup PMv2 requested={} accepted={} prior_compatible={} verified={} error={} owner_read={} device_absent={} window_absent={} windows_read={} process_windows={} process_before={:016X} thread_before={:016X} process_after={:016X} thread_after={:016X}; prepare before host graphics setup.",
            flag(startup.requested),
            flag(startup.accepted),
            flag(startup.prior_compatible),
            flag(startup.verified),
            startup.error,
            flag(owner_read),
            flag(owner_read && device == 0),
            flag(owner_read && window == 0),
            flag(startup.windows_read),
            startup.process_windows,
            startup.process_before,
            startup.thread_before,
            startup.process_after,
            startup.thread_after,
        ));
        crate::log_host(host, &text)?;
    }
    Ok(if !cold {
        Some(c"Fallout4 DPI startup refused: native owner unreadable or window/device already created.")
    } else if !startup.available {
        Some(c"Fallout4 DPI startup refused: required awareness APIs unavailable.")
    } else if !startup.prior_compatible && (!startup.windows_read || startup.process_windows != 0) {
        Some(c"Fallout4 DPI startup refused: window enumeration failed or a process window already exists.")
    } else if !startup.verified {
        Some(c"Fallout4 DPI startup refused: process or caller awareness is incompatible; existing thread awareness preserved.")
    } else {
        None
    })
}
