// SPDX-License-Identifier: MIT
//! The engine entries that route to the booted backend, the Vulkan boot, the Vulkan Present
//! markers and the WSI calls the engine driver makes outside the Runtime Vulkan host's guard.

use crate::sys::*;
use crate::{control, d3d12, window};
use rsf_game_sys::ffi_util::checked;
use rsf_game_sys::{RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_PRESENT, rsf_game_render_pass};
use std::cell::Cell;
use std::ffi::c_void;

type Handle = *mut c_void;

thread_local! {
    // Set only by an admitted Present boundary, on the thread that issues native WSI.
    static CAPTURE_NATIVE_PRESENT: Cell<bool> = const { Cell::new(false) };
}

/// The Runtime Vulkan host serves this process's session.
pub(crate) fn vulkan_booted() -> bool {
    unsafe { rsf_vkhost_booted() != 0 }
}

/// The Runtime Vulkan host's session, which binds the Runtime's Vulkan CPU service for the input
/// window.
///
/// # Safety
/// `setup` is null or a readable record of its stated size whose strings are NUL-terminated.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_boot_vulkan(setup: *const rsf_godot_setup) -> i32 {
    let result = unsafe { rsf_vkhost_boot(setup) };
    if result != 0 {
        window::bind_cpu_service(true);
    }
    result
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_render_begin(frame: u64) {
    if vulkan_booted() {
        unsafe { rsf_vkhost_render_begin(frame, 0) };
    } else if control::booted() {
        d3d12::render_begin(frame);
    }
}

/// While the Vulkan session is booted, asks the input window admission and reports the window as
/// the session's primary window.
#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_window(window: Handle) {
    if vulkan_booted() {
        window::accept(window);
        unsafe { rsf_vkhost_window(window) };
    }
}

/// Only the booted Vulkan session's admitted window marks its Present.
#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_present_marker(window: u64, begin: u32) -> i32 {
    if !vulkan_booted() || !window::admits_present(window) {
        return 0;
    }
    unsafe { rsf_latencyvk_present(begin) };
    CAPTURE_NATIVE_PRESENT.set(begin != 0);
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    if !vulkan_booted() {
        return 0;
    }
    let Some(pass) = (unsafe { checked(pass) }) else {
        return 0;
    };
    if pass.role == RSF_GAME_RENDER_FRAME {
        unsafe { rsf_vkhost_render_begin(pass.source_frame_id, 1) };
        return 1;
    }
    let admitted = window::execution(pass, begin);
    if admitted && pass.role == RSF_GAME_RENDER_PRESENT {
        CAPTURE_NATIVE_PRESENT.set(begin != 0);
    }
    admitted.into()
}

struct PresentCall {
    callback: rsf_vk_function,
    queue: Handle,
    info: *const c_void,
}

unsafe extern "C" fn present_call(user: Handle) -> i32 {
    let call = unsafe { &*user.cast::<PresentCall>() };
    unsafe { rsf_latencyvk_queue_present(call.callback, call.queue, call.info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_native_present(
    callback: u64,
    queue: Handle,
    info: *const c_void,
) -> i32 {
    // The engine supplies the native function address in a uint64_t wire field (Windows x64).
    let callback = unsafe { std::mem::transmute::<u64, rsf_vk_function>(callback) };
    let mut call = PresentCall {
        callback,
        queue,
        info,
    };
    if CAPTURE_NATIVE_PRESENT.get() {
        unsafe { rsf_vkhost_capture_present(info, 0, 1) };
    }
    let result =
        unsafe { rsf_slvk_without_dlssg(Some(present_call), std::ptr::from_mut(&mut call).cast()) };
    if CAPTURE_NATIVE_PRESENT.get() {
        unsafe { rsf_vkhost_capture_present(info, result, 0) };
    }
    result
}

struct WsiCall {
    callback: rsf_vk_wsi_call,
    user: Handle,
}

unsafe extern "C" fn wsi_call(user: Handle) -> i32 {
    let call = unsafe { &*user.cast::<WsiCall>() };
    unsafe { rsf_latencyvk_wsi_call(call.callback, call.user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_wsi_call(callback: rsf_vk_wsi_call, user: Handle) -> i32 {
    let mut call = WsiCall { callback, user };
    unsafe { rsf_slvk_without_dlssg(Some(wsi_call), std::ptr::from_mut(&mut call).cast()) }
}

/// The engine's own swapchain create or destroy for a window without a Runtime presenter: a native
/// secondary window (or a primary whose presenter was refused, which needs no DLSS-G either).
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_foreign_call(callback: rsf_vk_wsi_call, user: Handle) -> i32 {
    if vulkan_booted() {
        unsafe { rsf_vkhost_secondary_window() };
    }
    unsafe { rsf_slvk_without_dlssg(callback, user) }
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_shutdown() -> i32 {
    window::release();
    let result = if vulkan_booted() {
        unsafe { rsf_vkhost_shutdown() }
    } else {
        d3d12::shutdown()
    };
    if result != 0 {
        window::unbind_cpu_service();
    }
    result
}
