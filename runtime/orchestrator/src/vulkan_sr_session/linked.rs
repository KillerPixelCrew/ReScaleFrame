// SPDX-License-Identifier: MIT
//! The Runtime's Vulkan SR providers as the session's routes, for the Vulkan host
//! (vulkan_host.rs). The session's unit tests route to doubles instead.

use super::routes::{Route, Routes};
use super::{Fallback, Frame, Handle, Handoff, Setup};
use crate::sys::*;
use std::ffi::c_char;

static ROUTES: Routes = Routes {
    native: Route {
        open: native_open,
        plan: native_plan,
        record: native_record,
        retire: native_retire,
        close: native_close,
    },
    split: Route {
        open: split_open,
        plan: split_plan,
        record: split_record,
        retire: split_retire,
        close: split_close,
    },
    preset,
    fallback,
};

unsafe extern "C" fn native_open(setup: &Setup, _: *const Handoff, out: &mut Handle) -> i32 {
    let mut owner = std::ptr::null_mut();
    let result = unsafe { rsf_srvk_create(setup, &mut owner) };
    *out = owner.cast();
    result
}
unsafe extern "C" fn native_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe { rsf_srvk_plan(owner.cast(), width, height) }
}
unsafe extern "C" fn native_record(
    owner: Handle,
    commands: Handle,
    frame: *const Frame,
    _: &mut Handle,
) -> i32 {
    unsafe { rsf_srvk_evaluate(owner.cast(), commands, frame) }
}
unsafe extern "C" fn native_retire(owner: Handle, commands: Handle) {
    unsafe { rsf_srvk_retire_commands(owner.cast(), commands) };
}
unsafe extern "C" fn native_close(owner: Handle) -> i32 {
    unsafe { rsf_srvk_destroy(owner.cast()) }
}
unsafe extern "C" fn split_open(setup: &Setup, handoff: *const Handoff, out: &mut Handle) -> i32 {
    let mut owner = std::ptr::null_mut();
    let result = unsafe { rsf_srvk_fsr4_create(setup, handoff, &mut owner) };
    *out = owner.cast();
    result
}
unsafe extern "C" fn split_plan(owner: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe { rsf_srvk_fsr4_plan(owner.cast(), width, height) }
}
unsafe extern "C" fn split_record(
    owner: Handle,
    commands: Handle,
    frame: *const Frame,
    next: &mut Handle,
) -> i32 {
    unsafe { rsf_srvk_fsr4_evaluate(owner.cast(), commands, frame, next) }
}
unsafe extern "C" fn split_retire(owner: Handle, commands: Handle) {
    unsafe { rsf_srvk_fsr4_retire_commands(owner.cast(), commands) };
}
unsafe extern "C" fn split_close(owner: Handle) -> i32 {
    unsafe { rsf_srvk_fsr4_destroy(owner.cast()) }
}
unsafe extern "C" fn preset(owner: Handle, value: u32) {
    let _ = unsafe { rsf_srvk_dlss_preset(owner.cast(), value) };
}
unsafe extern "C" fn fallback(input: &Fallback) -> i32 {
    unsafe { rsf_srvk_spatial_fallback(input) }
}

pub(crate) unsafe fn open(
    setup: &Setup,
    handoff: *const Handoff,
    directory: *const c_char,
    preset: u32,
    out: &mut *mut super::Session,
) -> i32 {
    unsafe { super::rsf_core_srvk_session_open(&ROUTES, setup, handoff, directory, preset, out) }
}

pub(crate) unsafe fn record(
    owner: *mut super::Session,
    frame: *const Frame,
    fallback: &Fallback,
    split: bool,
    provider: *mut i32,
    next: *mut Handle,
) -> i32 {
    unsafe {
        super::rsf_core_srvk_session_record(
            &ROUTES,
            owner,
            frame,
            fallback,
            u32::from(split),
            provider,
            next,
        )
    }
}
