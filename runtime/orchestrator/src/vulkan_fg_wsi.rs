// SPDX-License-Identifier: MIT
use crate::vulkan_sr_abi::rsf_vk_function;
use crate::vulkan_timing::{timing_begin, timing_end};
use ash::vk::{self, Handle};
use std::ffi::c_void;

type Callback = unsafe extern "C" fn(*mut c_void) -> i32;
type Scope = unsafe extern "C" fn(Callback, *mut c_void) -> i32;
type QueuePresent = unsafe extern "C" fn(rsf_vk_function, *mut c_void, *const c_void) -> i32;

// Both native scopes are synchronous and invoke the borrowed closure at most once.
unsafe fn scoped<F: FnMut() -> vk::Result>(scope: Scope, mut invoke: F) -> vk::Result {
    unsafe extern "C" fn call<F: FnMut() -> vk::Result>(user: *mut c_void) -> i32 {
        unsafe { (&mut *user.cast::<F>())().as_raw() }
    }
    vk::Result::from_raw(unsafe { scope(call::<F>, (&mut invoke as *mut F).cast()) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkfg_wsi_create(
    without_fg: Scope,
    create: vk::PFN_vkCreateSwapchainKHR,
    device: vk::Device,
    info: *const vk::SwapchainCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    chain: *mut vk::SwapchainKHR,
) -> vk::Result {
    // The hooked command takes its own latency mutex.
    unsafe { scoped(without_fg, || create(device, info, allocator, chain)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkfg_wsi_destroy(
    without_fg: Scope,
    destroy: vk::PFN_vkDestroySwapchainKHR,
    device: vk::Device,
    chain: vk::SwapchainKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    unsafe {
        let _ = scoped(without_fg, || {
            destroy(device, chain, allocator);
            vk::Result::SUCCESS
        });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkfg_wsi_acquire(
    without_fg: Scope,
    synchronized: Scope,
    acquire: vk::PFN_vkAcquireNextImageKHR,
    device: vk::Device,
    chain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    unsafe {
        scoped(without_fg, || {
            scoped(synchronized, || {
                let start = timing_begin();
                let result = acquire(device, chain, timeout, semaphore, fence, index);
                timing_end("native_acquire", 0, start);
                result
            })
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkfg_wsi_images(
    without_fg: Scope,
    synchronized: Scope,
    images: vk::PFN_vkGetSwapchainImagesKHR,
    device: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    output: *mut vk::Image,
) -> vk::Result {
    unsafe {
        scoped(without_fg, || {
            scoped(synchronized, || images(device, chain, count, output))
        })
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vkfg_wsi_present(
    without_fg: Scope,
    synchronized_present: QueuePresent,
    present: vk::PFN_vkQueuePresentKHR,
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    unsafe {
        scoped(without_fg, || {
            let start = timing_begin();
            // The existing latency hook accepts an erased vkQueuePresentKHR.
            let callback =
                std::mem::transmute::<vk::PFN_vkQueuePresentKHR, unsafe extern "C" fn()>(present);
            let result = synchronized_present(
                Some(callback),
                queue.as_raw() as usize as *mut c_void,
                info.cast(),
            );
            timing_end("native_queue_present", 0, start);
            vk::Result::from_raw(result)
        })
    }
}

/// The caller lends a VkPresentInfoKHR and its swapchain array for the call.
pub(super) unsafe fn single_present_chain(info: *const c_void) -> Option<vk::SwapchainKHR> {
    let info = unsafe { info.cast::<vk::PresentInfoKHR<'_>>().as_ref() }?;
    if info.swapchain_count != 1 || info.p_swapchains.is_null() {
        return None;
    }
    Some(unsafe { info.p_swapchains.read() })
}
