// SPDX-License-Identifier: MIT
use crate::sys::rsf_fgvk_setup;
use crate::sys::rsf_vk_wsi_call;
use crate::sys::{rsf_vk_function, rsf_vk_get_proc};

/// A Streamline scope that runs a borrowed WSI callback (rsf-streamline-vk's bridge exports).
pub type WsiScope = Option<unsafe extern "C" fn(rsf_vk_wsi_call, *mut c_void) -> i32>;
/// Streamline's present that also carries the frame token.
pub type SynchronizedPresent =
    Option<unsafe extern "C" fn(usize, *mut c_void, *const c_void) -> i32>;
use crate::vulkan_dispatch::promoted_device_proc_alias;
use crate::vulkan_timing::{timing_begin, timing_end};
use ash::vk::{self, Handle};
use rsf_game_sys::backend_result::NOT_READY;
use std::ffi::{CStr, c_char, c_void};

const INIT_FAILED: i32 = vk::Result::ERROR_INITIALIZATION_FAILED.as_raw();

pub(crate) unsafe fn rsf_core_vkfg_device_proc(
    lookup: unsafe extern "C" fn(
        rsf_vk_get_proc,
        *mut c_void,
        *const c_char,
        *mut rsf_vk_function,
    ) -> i32,
    resolve: rsf_vk_get_proc,
    device: *mut c_void,
    name: *const c_char,
    entry: *mut rsf_vk_function,
) -> i32 {
    let result = unsafe { lookup(resolve, device, name, entry) };
    if result == 0
        && unsafe { (*entry).is_none() }
        && !name.is_null()
        && let Some(alias) = promoted_device_proc_alias(unsafe { CStr::from_ptr(name) })
    {
        return unsafe { lookup(resolve, device, alias.as_ptr(), entry) };
    }
    result
}

pub(crate) unsafe fn rsf_core_vkfg_graphics_submit(
    setup: &rsf_fgvk_setup,
    count: u32,
    submits: *const vk::SubmitInfo<'_>,
    fence: vk::Fence,
) -> vk::Result {
    let submit = setup
        .graphics_submit
        .expect("validated graphics submit callback");
    vk::Result::from_raw(unsafe { submit(setup.queue_user, count, submits.cast(), fence.as_raw()) })
}

pub(crate) unsafe fn rsf_core_vkfg_drain(
    setup: &rsf_fgvk_setup,
    wait_for_presents: Callback,
    user: *mut c_void,
    idle: vk::PFN_vkDeviceWaitIdle,
) -> i32 {
    let lock = setup.lock_queues.expect("validated queue lock");
    let unlock = setup.unlock_queues.expect("validated queue unlock");
    unsafe { lock(setup.queue_user) };
    let ready = unsafe { wait_for_presents(user) } == 0
        && unsafe { idle(vk::Device::from_raw(setup.graphics.device as usize as u64)) }
            == vk::Result::SUCCESS;
    unsafe { unlock(setup.queue_user) };
    if ready { 0 } else { NOT_READY }
}

pub unsafe fn rsf_core_slvk_without_dlssg(
    scope: WsiScope,
    callback: rsf_vk_wsi_call,
    user: *mut c_void,
) -> i32 {
    let Some(call) = callback else {
        return INIT_FAILED;
    };
    match scope {
        Some(scope) => unsafe { scope(callback, user) },
        None => unsafe { call(user) },
    }
}

pub unsafe fn rsf_core_latencyvk_instance_proc(
    resolve: rsf_vk_get_proc,
    instance: *mut c_void,
    name: *const c_char,
) -> rsf_vk_function {
    let resolve = resolve?;
    if name.is_null() {
        return None;
    }
    unsafe { resolve(instance, name) }
}

pub unsafe fn rsf_core_latencyvk_queue_present(
    synchronized: SynchronizedPresent,
    callback: rsf_vk_function,
    queue: *mut c_void,
    info: *const c_void,
) -> i32 {
    let Some(callback) = callback else {
        return INIT_FAILED;
    };
    if queue.is_null() || info.is_null() {
        return INIT_FAILED;
    }
    if let Some(synchronized) = synchronized {
        return unsafe { synchronized(callback as usize, queue, info) };
    }

    // The public C ABI erases Vulkan function types. This entry only accepts
    // vkQueuePresentKHR and borrows the original queue and present info.
    let present = unsafe {
        std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkQueuePresentKHR>(callback)
    };
    unsafe { present(vk::Queue::from_raw(queue as usize as u64), info.cast()).as_raw() }
}

pub unsafe fn rsf_core_latencyvk_wsi_call(
    scope: WsiScope,
    callback: rsf_vk_wsi_call,
    user: *mut c_void,
) -> i32 {
    if user.is_null() {
        return INIT_FAILED;
    }
    unsafe { rsf_core_slvk_without_dlssg(scope, callback, user) }
}

pub(crate) type Callback = unsafe extern "C" fn(*mut c_void) -> i32;
pub(crate) type Scope = unsafe extern "C" fn(Callback, *mut c_void) -> i32;
pub(crate) type QueuePresent =
    unsafe extern "C" fn(rsf_vk_function, *mut c_void, *const c_void) -> i32;

// Both native scopes are synchronous and invoke the borrowed closure at most once.
unsafe fn scoped<F: FnMut() -> vk::Result>(scope: Scope, mut invoke: F) -> vk::Result {
    unsafe extern "C" fn call<F: FnMut() -> vk::Result>(user: *mut c_void) -> i32 {
        unsafe { (&mut *user.cast::<F>())().as_raw() }
    }
    vk::Result::from_raw(unsafe { scope(call::<F>, (&mut invoke as *mut F).cast()) })
}

pub(crate) unsafe fn rsf_core_vkfg_wsi_create(
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

pub(crate) unsafe fn rsf_core_vkfg_wsi_destroy(
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

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn rsf_core_vkfg_wsi_acquire(
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

pub(crate) unsafe fn rsf_core_vkfg_wsi_images(
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

pub(crate) unsafe fn rsf_core_vkfg_wsi_present(
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
