// SPDX-License-Identifier: MIT
//! Skyrim Special Edition (Steam 1.7.104.0) adapter policy linked into
//! `ReScaleFrame.Game.SkyrimSE.dll`. Rust owns the GameSDK table and observer admission
//! (`plugin.rs`), and cold native creation with Steam Present cooperation (`creation.rs`).
//! Native code lends the typed callbacks and still owns the renderer lifecycle.
mod creation;
mod layout;
mod observers;
mod platform;
mod plugin;

use platform::{CreationCall, Platform};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::c_void;

/// Prepares the disabled creation hook and installs the cold-only Steam forwarding.
///
/// # Safety
/// `host` is null or valid for the call; `platform` is null or a process-lived table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_creation_prepare(
    host: *const rsf_game_host_services,
    platform: *const Platform,
) -> bool {
    let Some(platform) = (unsafe { platform::register(platform) }) else {
        return false;
    };
    match unsafe { host.as_ref() } {
        Some(host) => creation::prepare(platform, host),
        None => false,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_creation_start() -> bool {
    creation::start()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_creation_quiesce() {
    creation::quiesce();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_creation_stop() -> bool {
    creation::stop()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_skyrim_creation_busy() -> bool {
    creation::busy()
}

/// Called by the detour before the original routine, which it returns.
///
/// # Safety
/// `call` is null or the detour's record, exclusively borrowed until the matching leave.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_creation_enter(call: *mut CreationCall) -> *mut c_void {
    let original = creation::enter_lease();
    if let (Some(platform), Some(call)) = (platform::get(), unsafe { call.as_mut() })
        && call.valid()
    {
        creation::enter(platform, call);
    }
    original
}

/// Called by the detour after the original returned or unwound; mandatory after enter.
///
/// # Safety
/// `call` is null or the record passed to the matching enter.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_creation_leave(call: *mut CreationCall) {
    if let (Some(platform), Some(call)) = (platform::get(), unsafe { call.as_mut() })
        && call.valid()
    {
        creation::leave(platform, call);
    }
    creation::leave_lease();
}
