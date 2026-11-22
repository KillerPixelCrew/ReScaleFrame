// SPDX-License-Identifier: MIT
//! VulkanStory package aliases for the standalone FSR3 ABI.
#![allow(non_snake_case)]

use super::*;

#[unsafe(no_mangle)]
extern "C" fn VulkanStoryFsr3Open(runtime: Handle) -> i32 {
    exports::rsf_vk_fsr3_open(runtime)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3Plan(
    width: u32,
    height: u32,
    quality: u32,
    render_width: *mut u32,
    render_height: *mut u32,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_plan(width, height, quality, render_width, render_height) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3Create(
    device: Handle,
    physical: Handle,
    render_width: u32,
    render_height: u32,
    width: u32,
    height: u32,
    result: *mut *mut Context,
) -> i32 {
    unsafe {
        exports::rsf_vk_fsr3_create(
            device,
            physical,
            render_width,
            render_height,
            width,
            height,
            result,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3EvaluateWithComposition(
    context: *mut Context,
    frame: *const Frame,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_evaluate_with_composition(context, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3Destroy(context: *mut Context) -> i32 {
    unsafe { exports::rsf_vk_fsr3_destroy(context) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainCreate(
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
    unsafe {
        exports::rsf_vk_fsr3_swapchain_create(
            physical,
            device,
            game,
            async_compute,
            present,
            acquire,
            queue_family,
            info,
            result,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainHandle(owner: *mut Swapchain) -> Vk {
    unsafe { exports::rsf_vk_fsr3_swapchain_handle(owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainRecreate(
    owner: *mut Swapchain,
    info: *const u8,
    chain: *mut Vk,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_recreate(owner, info, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainGetImages(
    owner: *mut Swapchain,
    chain: Vk,
    count: *mut u32,
    images: *mut Vk,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_get_images(owner, chain, count, images) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainAcquire(
    owner: *mut Swapchain,
    chain: Vk,
    timeout: u64,
    semaphore: Vk,
    fence: Vk,
    index: *mut u32,
) -> i32 {
    unsafe {
        exports::rsf_vk_fsr3_swapchain_acquire(owner, chain, timeout, semaphore, fence, index)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainPresent(
    owner: *mut Swapchain,
    queue: Handle,
    info: *const c_void,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_present(owner, queue, info) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainPrepareDestroy(
    owner: *mut Swapchain,
    chain: Vk,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_prepare_destroy(owner, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainDestroyChainChecked(
    owner: *mut Swapchain,
    chain: Vk,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_destroy_chain_checked(owner, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainDestroyChain(owner: *mut Swapchain, chain: Vk) {
    unsafe { exports::rsf_vk_fsr3_swapchain_destroy_chain(owner, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainLastPresentCount(
    owner: *mut Swapchain,
    chain: Vk,
) -> u64 {
    unsafe { exports::rsf_vk_fsr3_swapchain_last_present_count(owner, chain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3SwapchainDestroy(owner: *mut Swapchain) -> i32 {
    unsafe { exports::rsf_vk_fsr3_swapchain_destroy(owner) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3FgCreate(
    device: Handle,
    physical: Handle,
    width: u32,
    height: u32,
    back_buffer_format: i32,
    result: *mut *mut FgContext,
) -> i32 {
    unsafe {
        exports::rsf_vk_fsr3_fg_create(device, physical, width, height, back_buffer_format, result)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3FgDisable(
    context: *mut FgContext,
    swapchain: *mut Swapchain,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_fg_disable(context, swapchain) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3FgEvaluate(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_fg_evaluate(context, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3FgEvaluateDirect(
    context: *mut FgContext,
    frame: *const FgFrame,
) -> i32 {
    unsafe { exports::rsf_vk_fsr3_fg_evaluate_direct(context, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn VulkanStoryFsr3FgDestroy(context: *mut FgContext) -> i32 {
    unsafe { exports::rsf_vk_fsr3_fg_destroy(context) }
}
