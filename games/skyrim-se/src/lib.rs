// SPDX-License-Identifier: MIT
//! Skyrim Special Edition (Steam 1.7.104.0) adapter policy linked into
//! `ReScaleFrame.Game.SkyrimSE.dll`. Rust owns the GameSDK table and observer admission
//! (`plugin.rs`), the lifecycle with its process-lived Runtime session, DPI startup and subsystem
//! order (`session.rs`), the observer hook transactions (`observers.rs`) and cold native creation
//! with Steam Present cooperation (`creation.rs`). Rust also owns the CPU hook lifecycle
//! (`cpu_hooks.rs`), event records, scoped render facts (`cpu_policy.rs`), native job and poll proof,
//! and prospective source transitions (`cpu_sources.rs`). Native code keeps typed detours,
//! thread-local scopes, guarded reads and MinHook.
mod auxiliary;
mod camera;
mod compatibility;
mod cpu_hooks;
mod cpu_policy;
mod cpu_sources;
mod creation;
mod hook_set;
mod layout;
mod observers;
mod platform;
mod plugin;
mod postprocess;
mod presentation;
mod render_hook;
mod render_inputs;
mod render_observer;
mod resolution;
mod sampling;
mod session;
mod taa;

use platform::CreationCall;
use std::ffi::c_void;

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
