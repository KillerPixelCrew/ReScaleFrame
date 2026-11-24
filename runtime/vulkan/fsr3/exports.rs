// SPDX-License-Identifier: MIT
//! Stable VulkanStory FSR3 DLL entry points consumed by the managed host.

use super::*;
use std::io::Write;

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_vk_fsr3_open(runtime: Handle) -> i32 {
    unsafe { rsf_core_fsr3_open(runtime) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_plan(
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    unsafe { rsf_core_fsr3_plan(width, height, quality, render_width, render_height) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_create(
    device: Handle,
    physical: Handle,
    render_width: u32,
    render_height: u32,
    width: u32,
    height: u32,
    result: *mut *mut Context,
) -> i32 {
    let request = Request {
        device,
        physical,
        render_width,
        render_height,
        width,
        height,
    };
    unsafe { rsf_core_fsr3_create(&request, result) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_evaluate_with_composition(
    context: *mut Context,
    frame: *const Frame,
) -> i32 {
    unsafe { rsf_core_fsr3_evaluate(context, frame) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_destroy(context: *mut Context) -> i32 {
    unsafe { rsf_core_fsr3_destroy(context) }
}

/// A failed rollback can return a cleanup-only owner for SwapchainDestroy.
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_create(
    physical: Handle,
    device: Handle,
    game: Handle,
    async_compute: Handle,
    present: Handle,
    acquire: Handle,
    queue_family: u32,
    info: *const u8,
    result: *mut *mut Swapchain,
) -> i32 {
    let request = WsiRequest {
        physical,
        device,
        game,
        async_compute,
        present,
        acquire,
        info,
        queue_family,
    };
    unsafe { rsf_core_fsr3_swapchain_create(&request, result) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_handle(owner: *mut Swapchain) -> Vk {
    unsafe { rsf_core_fsr3_swapchain_handle(owner) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_recreate(
    owner: *mut Swapchain,
    info: *const u8,
    chain: *mut Vk,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_recreate(owner, info, chain) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_get_images(
    owner: *mut Swapchain,
    chain: Vk,
    count: *mut u32,
    images: *mut Vk,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_get_images(owner, chain, count, images) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_acquire(
    owner: *mut Swapchain,
    chain: Vk,
    timeout: u64,
    semaphore: Vk,
    fence: Vk,
    index: *mut u32,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_acquire(owner, chain, timeout, semaphore, fence, index) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_present(
    owner: *mut Swapchain,
    queue: Handle,
    info: *const c_void,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_present(owner, queue, info) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_prepare_destroy(
    owner: *mut Swapchain,
    chain: Vk,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_prepare_destroy(owner, chain) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy_chain_checked(
    owner: *mut Swapchain,
    chain: Vk,
) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_destroy_chain(owner, chain) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy_chain(
    owner: *mut Swapchain,
    chain: Vk,
) {
    let code = unsafe { rsf_core_fsr3_swapchain_destroy_chain(owner, chain) };
    if code != 0 {
        let _ = writeln!(
            std::io::stderr().lock(),
            "[FidelityFX] swapchain destruction retained ownership after error {code}"
        );
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_last_present_count(
    owner: *mut Swapchain,
    chain: Vk,
) -> u64 {
    unsafe { rsf_core_fsr3_swapchain_last_present_count(owner, chain) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_swapchain_destroy(owner: *mut Swapchain) -> i32 {
    unsafe { rsf_core_fsr3_swapchain_destroy(owner) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_fg_create(
    device: Handle,
    physical: Handle,
    width: u32,
    height: u32,
    back_buffer_format: i32,
    result: *mut *mut FgContext,
) -> i32 {
    let request = FgRequest {
        device,
        physical,
        width,
        height,
        back_buffer_format,
    };
    unsafe { rsf_core_fsr3_fg_create(&request, result) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_fg_disable(
    context: *mut FgContext,
    swapchain: *mut Swapchain,
) -> i32 {
    unsafe { rsf_core_fsr3_fg_disable(context, swapchain) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_fg_evaluate(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    unsafe { rsf_core_fsr3_fg_evaluate(context, frame) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_fg_evaluate_direct(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    unsafe { rsf_core_fsr3_fg_evaluate_direct(context, frame) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_vk_fsr3_fg_destroy(context: *mut FgContext) -> i32 {
    unsafe { rsf_core_fsr3_fg_destroy(context) }
}
