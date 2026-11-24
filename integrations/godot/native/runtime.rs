// SPDX-License-Identifier: MIT
//! The engine entries that route to the booted backend, the Vulkan Present markers and the WSI
//! calls the engine driver makes outside the Vulkan session's guard.

use crate::sys::*;
use crate::vulkan_creation::VkFunction;
use crate::{control, d3d12, vulkan, window};
use rsf_game_sys::ffi_util::checked;
use rsf_game_sys::{RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_PRESENT, rsf_game_render_pass};
use std::cell::Cell;
use std::ffi::c_void;

type Handle = *mut c_void;

thread_local! {
    // Set only by an admitted Present boundary, on the thread that issues native WSI.
    static CAPTURE_NATIVE_PRESENT: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_render_begin(frame: u64) {
    if vulkan::booted() {
        vulkan::render_begin(frame, false);
    } else if control::booted() {
        d3d12::render_begin(frame);
    }
}

/// While the Vulkan session is booted, asks the input window admission and keeps the window as
/// the session's primary window.
#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_window(window: Handle) {
    if vulkan::booted() {
        window::accept(window);
        vulkan::report_window(window);
    }
}

/// Only the booted Vulkan session's admitted window marks its Present.
#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_present_marker(window: u64, begin: u32) -> i32 {
    if !vulkan::booted() || !window::admits_present(window) {
        return 0;
    }
    unsafe { rsf_latencyvk_present(begin) };
    CAPTURE_NATIVE_PRESENT.set(begin != 0);
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    if !vulkan::booted() {
        return 0;
    }
    let Some(pass) = (unsafe { checked(pass) }) else {
        return 0;
    };
    if pass.role == RSF_GAME_RENDER_FRAME {
        vulkan::render_begin(pass.source_frame_id, true);
        return 1;
    }
    let admitted = window::execution(pass, begin);
    if admitted && pass.role == RSF_GAME_RENDER_PRESENT {
        CAPTURE_NATIVE_PRESENT.set(begin != 0);
    }
    admitted.into()
}

struct PresentCall {
    callback: Option<VkFunction>,
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
    let callback = unsafe { std::mem::transmute::<u64, Option<VkFunction>>(callback) };
    let mut call = PresentCall {
        callback,
        queue,
        info,
    };
    if CAPTURE_NATIVE_PRESENT.get() {
        vulkan::capture_present(info, 0, true);
    }
    let result =
        unsafe { rsf_slvk_without_dlssg(Some(present_call), std::ptr::from_mut(&mut call).cast()) };
    if CAPTURE_NATIVE_PRESENT.get() {
        vulkan::capture_present(info, result, false);
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_foreign_call(callback: rsf_vk_wsi_call, user: Handle) -> i32 {
    unsafe { rsf_slvk_without_dlssg(callback, user) }
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_shutdown() -> i32 {
    window::release();
    let result = if vulkan::booted() {
        vulkan::shutdown()
    } else {
        d3d12::shutdown()
    };
    if result != 0 {
        window::unbind_cpu_service();
    }
    result
}
