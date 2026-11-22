// SPDX-License-Identifier: MIT
//! Engine ABI entry points backed by the existing Rust session and execution receipts.

use crate::platform::{Settings, Setup};
use crate::sys::*;
use crate::vulkan::native::{rsf_godot_vk_capture_present, rsf_godot_vk_render_begin};
use rsf_game_sys::{RSF_GAME_RENDER_FRAME, RSF_GAME_RENDER_PRESENT, rsf_game_render_pass};
use std::cell::Cell;
use std::ffi::c_void;

type Handle = *mut c_void;

thread_local! {
    // Set only by an admitted Present boundary, on the thread that issues native WSI.
    static CAPTURE_NATIVE_PRESENT: Cell<bool> = const { Cell::new(false) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_boot(setup: *const Setup) -> i32 {
    unsafe { crate::rsf_godot_boot(setup) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_create_swapchain(
    factory: Handle,
    queue: Handle,
    window: Handle,
    description: *const c_void,
    out: *mut Handle,
) -> i32 {
    if !crate::CONTROL.booted() {
        return windows::Win32::Graphics::Dxgi::DXGI_ERROR_UNSUPPORTED.0;
    }
    unsafe { rsf_d3d11_present_create_for_hwnd(factory, queue, window, description, out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_configure(settings: *const Settings) -> i32 {
    unsafe { crate::rsf_godot_configure(settings) }
}

/// The engine's declared size word is safe to read before borrowing the full pass.
unsafe fn sized_pass<'a>(pass: *const rsf_game_render_pass) -> Option<&'a rsf_game_render_pass> {
    (!pass.is_null()
        && unsafe { pass.cast::<u32>().read() } as usize >= size_of::<rsf_game_render_pass>())
    .then(|| unsafe { &*pass })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_render(
    device: Handle,
    queue: Handle,
    commands: Handle,
    pass: *const rsf_game_render_pass,
) -> i32 {
    if !crate::CONTROL.booted() || device.is_null() || queue.is_null() || commands.is_null() {
        return 0;
    }
    let Some(pass) = (unsafe { sized_pass(pass) }) else {
        return 0;
    };
    if rsf_game_sys::depth_domains_requested(&pass.depth_domains) {
        return 0;
    }
    crate::CONTROL
        .d3d12
        .render(&crate::CONTROL, device, queue, commands, pass)
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_render_begin(frame: u64) {
    if crate::CONTROL.vulkan.booted() {
        rsf_godot_vk_render_begin(frame, 0);
    } else if crate::CONTROL.booted() {
        crate::CONTROL.d3d12.render_begin(&crate::CONTROL, frame);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsfg_vk_present_marker(window: u64, begin: u32) -> i32 {
    let admitted: i32 = crate::CONTROL
        .window
        .present_marker(crate::CONTROL.vulkan.booted(), window, begin)
        .into();
    if admitted != 0 {
        CAPTURE_NATIVE_PRESENT.set(begin != 0);
    }
    admitted
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_vk_execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    if !crate::CONTROL.vulkan.booted() {
        return 0;
    }
    let Some(pass) = (unsafe { sized_pass(pass) }) else {
        return 0;
    };
    if pass.role == RSF_GAME_RENDER_FRAME {
        rsf_godot_vk_render_begin(pass.source_frame_id, 1);
        return 1;
    }
    let admitted: i32 = crate::CONTROL.window.execution(pass, begin).into();
    if admitted != 0 && pass.role == RSF_GAME_RENDER_PRESENT {
        CAPTURE_NATIVE_PRESENT.set(begin != 0);
    }
    admitted
}

struct PresentCall {
    callback: Option<crate::platform::VkFunction>,
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
    let callback =
        unsafe { std::mem::transmute::<u64, Option<crate::platform::VkFunction>>(callback) };
    let mut call = PresentCall {
        callback,
        queue,
        info,
    };
    if CAPTURE_NATIVE_PRESENT.get() {
        rsf_godot_vk_capture_present(info, 0, 1);
    }
    let result =
        unsafe { rsf_slvk_without_dlssg(Some(present_call), std::ptr::from_mut(&mut call).cast()) };
    if CAPTURE_NATIVE_PRESENT.get() {
        rsf_godot_vk_capture_present(info, result, 0);
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
    let control = &crate::CONTROL;
    control.window.release();
    let result = if control.vulkan.booted() {
        control.vulkan.shutdown()
    } else {
        control.d3d12.shutdown(control)
    };
    if result != 0 {
        control.window.bind_cpu_service(None);
    }
    result
}

/// `ReScaleFrameRuntime.request_frame_dump`, on the render thread.
pub(crate) fn rsf_godot_extension_capture() {
    unsafe { rsf_input_dump_request() };
}
