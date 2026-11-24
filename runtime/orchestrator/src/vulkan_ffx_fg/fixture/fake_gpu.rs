// SPDX-License-Identifier: MIT
//! Fake Vulkan device, queue and WSI entries for the FFX fixture, and the state it observes.
use super::fake_sdk::{rsf_fixture_ffx_chain, rsf_fixture_ffx_routed};
use super::rsf_fixture_ffx_refused_second_owner;
use crate::sys::rsf_vk_function;
use crate::vulkan_ffx_fg::{Owner, rsf_ffxgvk_destroy};
use ash::vk::{self, Handle};
use rsf_test_support::ignore_pipeline_barrier;
use std::{
    cell::UnsafeCell,
    ffi::{CStr, c_char, c_void},
};

/// What the FFX scenarios steer and observe.
pub(super) struct State {
    pub(super) next: u64,
    pub(super) resets: u32,
    pub(super) submits: u32,
    pub(super) waits: u32,
    pub(super) copies: u32,
    pub(super) sdk_records: u32,
    pub(super) generated: u32,
    pub(super) configurations: u32,
    pub(super) activations: u32,
    pub(super) present_calls: u32,
    pub(super) drains: u32,
    pub(super) idles: u32,
    pub(super) locks: u32,
    pub(super) unlocks: u32,
    pub(super) effects_destroyed: u32,
    pub(super) swapchains_destroyed: u32,
    pub(super) chains_destroyed: u32,
    pub(super) destroyed_images: u32,
    pub(super) freed_memories: u32,
    pub(super) destroyed_fences: u32,
    pub(super) reject_prepare: bool,
    pub(super) reject_wait: bool,
    pub(super) reject_bind: bool,
    pub(super) reject_drain: bool,
    pub(super) reject_effect_destroy: bool,
    pub(super) reject_swapchain_destroy: bool,
    pub(super) reject_generate: bool,
    pub(super) check_reset: bool,
    pub(super) expected_reset: bool,
    pub(super) enabled: bool,
    pub(super) reject_submit: bool,
    pub(super) configured_frame: u64,
    pub(super) generated_frame: u64,
    pub(super) present_result: i32,
}

/// What the WSI route scenario steers and observes.
pub(super) struct RouteState {
    pub(super) owner: *mut Owner,
    pub(super) presents: u32,
    pub(super) creates: u32,
    pub(super) destroys: u32,
    pub(super) acquires: u32,
    pub(super) enumerations: u32,
    pub(super) lookups: u32,
    pub(super) unregistered: u32,
    pub(super) registered: u32,
    pub(super) destroy_inside: bool,
    pub(super) refuse_unregister: bool,
    pub(super) destroyed_inside: i32,
    pub(super) created_inside: i32,
    pub(super) routed_inside: *mut c_void,
}

// Scenario, SDK callbacks and fake Vulkan calls all execute on the initiating
// thread. Use raw pointers because Present can synchronously reenter teardown.
thread_local! {
    static FIXTURE: UnsafeCell<State> = const { UnsafeCell::new(unsafe { std::mem::zeroed() }) };
    static ROUTE: UnsafeCell<RouteState> = const { UnsafeCell::new(unsafe { std::mem::zeroed() }) };
}

pub(super) fn rsf_fixture_ffx_state() -> *mut State {
    state()
}

pub(super) fn rsf_fixture_wsi_route_state() -> *mut RouteState {
    route()
}

pub(super) unsafe extern "system" fn rsf_fixture_ffx_images(
    _: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    images: *mut vk::Image,
) -> vk::Result {
    assert_eq!(chain.as_raw(), rsf_fixture_ffx_chain());
    const IMAGE_COUNT: u32 = 3;
    // The pinned SDK leaves oversized counts unchanged on a fill.
    if images.is_null() {
        unsafe { count.write(IMAGE_COUNT) };
    } else {
        for index in 0..unsafe { count.read() }.min(IMAGE_COUNT) {
            unsafe {
                images
                    .add(index as usize)
                    .write(vk::Image::from_raw(u64::from(index) + 1000))
            };
        }
    }
    if unsafe { count.read() } < IMAGE_COUNT {
        vk::Result::INCOMPLETE
    } else {
        vk::Result::SUCCESS
    }
}

pub(super) unsafe extern "system" fn rsf_fixture_ffx_acquire(
    _: vk::Device,
    _: vk::SwapchainKHR,
    _: u64,
    _: vk::Semaphore,
    _: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    unsafe { index.write(1) };
    vk::Result::SUCCESS
}

pub(super) extern "C" fn rsf_fixture_ffx_present_count(_: vk::SwapchainKHR) -> u64 {
    u64::from(unsafe { (*state()).present_calls })
}

// The FFX scenarios and their fake queue calls run on one thread.
pub(super) fn state() -> *mut State {
    FIXTURE.with(UnsafeCell::get)
}

fn next() -> u64 {
    let state = state();
    unsafe {
        (*state).next += 1;
        (*state).next
    }
}

fn route() -> *mut RouteState {
    ROUTE.with(UnsafeCell::get)
}

unsafe extern "system" fn wsi_present(
    queue: vk::Queue,
    _: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    assert_eq!(
        queue.as_raw(),
        22,
        "the hook forwards the SDK present queue"
    );
    let route = route();
    unsafe {
        (*route).presents += 1;
        if (*route).destroy_inside {
            (*route).destroyed_inside = rsf_ffxgvk_destroy((*route).owner);
            (*route).routed_inside = rsf_fixture_ffx_routed();
            (*route).created_inside = rsf_fixture_ffx_refused_second_owner();
        }
    }
    vk::Result::SUBOPTIMAL_KHR
}

unsafe extern "system" fn wsi_create(
    _: vk::Device,
    _: *const vk::SwapchainCreateInfoKHR<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    chain: *mut vk::SwapchainKHR,
) -> vk::Result {
    unsafe {
        chain.write(vk::SwapchainKHR::from_raw(40));
        (*route()).creates += 1;
    }
    vk::Result::SUCCESS
}

unsafe extern "system" fn wsi_destroy(
    _: vk::Device,
    chain: vk::SwapchainKHR,
    _: *const vk::AllocationCallbacks<'_>,
) {
    assert_eq!(chain.as_raw(), 40, "the hook forwards the destroyed chain");
    unsafe { (*route()).destroys += 1 };
}

unsafe extern "system" fn wsi_acquire(
    _: vk::Device,
    _: vk::SwapchainKHR,
    _: u64,
    _: vk::Semaphore,
    _: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    unsafe {
        index.write(2);
        (*route()).acquires += 1;
    }
    vk::Result::SUCCESS
}

unsafe extern "system" fn wsi_images(
    _: vk::Device,
    _: vk::SwapchainKHR,
    count: *mut u32,
    _: *mut vk::Image,
) -> vk::Result {
    unsafe {
        count.write(3);
        (*route()).enumerations += 1;
    }
    vk::Result::INCOMPLETE
}

// Initial route population queries this table directly. For subsequent
// backend queries the loader below verifies promoted-name fallback.
pub(super) unsafe extern "C" fn rsf_fixture_wsi_entry(name: *const c_char) -> rsf_vk_function {
    macro_rules! entry {
        ($function:ident, $signature:ident) => {
            Some(unsafe {
                std::mem::transmute::<vk::$signature, unsafe extern "C" fn()>($function)
            })
        };
    }
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkCreateSwapchainKHR" => entry!(wsi_create, PFN_vkCreateSwapchainKHR),
        b"vkDestroySwapchainKHR" => entry!(wsi_destroy, PFN_vkDestroySwapchainKHR),
        b"vkQueuePresentKHR" => entry!(wsi_present, PFN_vkQueuePresentKHR),
        b"vkAcquireNextImageKHR" => entry!(wsi_acquire, PFN_vkAcquireNextImageKHR),
        b"vkGetSwapchainImagesKHR" | b"vkBindImageMemory2" => {
            entry!(wsi_images, PFN_vkGetSwapchainImagesKHR)
        }
        _ => None,
    }
}

pub(super) unsafe extern "C" fn rsf_fixture_wsi_lookup(
    _: *mut c_void,
    name: *const c_char,
) -> rsf_vk_function {
    unsafe {
        (*route()).lookups += 1;
        if CStr::from_ptr(name) == c"vkBindImageMemory2" {
            rsf_fixture_wsi_entry(name)
        } else {
            None
        }
    }
}

unsafe extern "system" fn create_fence(
    _: vk::Device,
    _: *const vk::FenceCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Fence,
) -> vk::Result {
    unsafe { out.write(vk::Fence::from_raw(next())) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_fence(
    _: vk::Device,
    _: vk::Fence,
    _: *const vk::AllocationCallbacks<'_>,
) {
    unsafe { (*state()).destroyed_fences += 1 };
}

unsafe extern "system" fn reset_fences(
    _: vk::Device,
    count: u32,
    fence: *const vk::Fence,
) -> vk::Result {
    assert_eq!(count, 1);
    assert!(
        !unsafe { fence.read() }.is_null(),
        "reset has a real completion identity"
    );
    unsafe { (*state()).resets += 1 };
    vk::Result::SUCCESS
}

unsafe extern "system" fn wait_fences(
    _: vk::Device,
    count: u32,
    fence: *const vk::Fence,
    all: vk::Bool32,
    timeout: u64,
) -> vk::Result {
    assert_eq!((count, all, timeout), (1, vk::TRUE, 15_000_000_000));
    assert!(
        !unsafe { fence.read() }.is_null(),
        "wait observes the earlier completion"
    );
    let state = state();
    unsafe {
        (*state).waits += 1;
        if (*state).reject_wait {
            vk::Result::TIMEOUT
        } else {
            vk::Result::SUCCESS
        }
    }
}

pub(super) extern "C" fn rsf_fixture_ffx_submit(
    _: *mut c_void,
    count: u32,
    submits: *const c_void,
    fence: u64,
) -> i32 {
    assert!(
        count == 0 && submits.is_null() && fence != 0,
        "completion submits use the caller's fence"
    );
    let state = state();
    unsafe {
        (*state).submits += 1;
        if (*state).reject_submit {
            vk::Result::ERROR_DEVICE_LOST
        } else {
            vk::Result::SUCCESS
        }
    }
    .as_raw()
}

pub(super) extern "C" fn rsf_fixture_ffx_lock(_: *mut c_void) {
    unsafe { (*state()).locks += 1 };
}
pub(super) extern "C" fn rsf_fixture_ffx_unlock(_: *mut c_void) {
    unsafe { (*state()).unlocks += 1 };
}

unsafe extern "system" fn create_image(
    _: vk::Device,
    _: *const vk::ImageCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Image,
) -> vk::Result {
    unsafe { out.write(vk::Image::from_raw(next())) };
    vk::Result::SUCCESS
}
unsafe extern "system" fn destroy_image(
    _: vk::Device,
    _: vk::Image,
    _: *const vk::AllocationCallbacks<'_>,
) {
    unsafe { (*state()).destroyed_images += 1 };
}
unsafe extern "system" fn requirements(
    _: vk::Device,
    _: vk::Image,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 4096,
            alignment: 0,
            memory_type_bits: 1,
        })
    };
}
unsafe extern "system" fn allocate(
    _: vk::Device,
    _: *const vk::MemoryAllocateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::DeviceMemory,
) -> vk::Result {
    unsafe { out.write(vk::DeviceMemory::from_raw(next())) };
    vk::Result::SUCCESS
}
unsafe extern "system" fn free_memory(
    _: vk::Device,
    _: vk::DeviceMemory,
    _: *const vk::AllocationCallbacks<'_>,
) {
    unsafe { (*state()).freed_memories += 1 };
}
unsafe extern "system" fn bind(
    _: vk::Device,
    _: vk::Image,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    if unsafe { (*state()).reject_bind } {
        vk::Result::ERROR_UNKNOWN
    } else {
        vk::Result::SUCCESS
    }
}

unsafe extern "system" fn copy(
    _: vk::CommandBuffer,
    _: vk::Image,
    _: vk::ImageLayout,
    _: vk::Image,
    _: vk::ImageLayout,
    _: u32,
    _: *const vk::ImageCopy,
) {
    unsafe { (*state()).copies += 1 };
}
unsafe extern "system" fn idle(_: vk::Device) -> vk::Result {
    unsafe { (*state()).idles += 1 };
    vk::Result::SUCCESS
}

pub(super) unsafe extern "C" fn rsf_fixture_ffx_lookup(
    _: *mut c_void,
    name: *const c_char,
) -> rsf_vk_function {
    macro_rules! function {
        ($function:ident, $signature:ident) => {
            Some(unsafe {
                std::mem::transmute::<vk::$signature, unsafe extern "C" fn()>($function)
            })
        };
    }
    match unsafe { CStr::from_ptr(name) }.to_bytes() {
        b"vkCreateImage" => function!(create_image, PFN_vkCreateImage),
        b"vkDestroyImage" => function!(destroy_image, PFN_vkDestroyImage),
        b"vkGetImageMemoryRequirements" => {
            function!(requirements, PFN_vkGetImageMemoryRequirements)
        }
        b"vkAllocateMemory" => function!(allocate, PFN_vkAllocateMemory),
        b"vkFreeMemory" => function!(free_memory, PFN_vkFreeMemory),
        b"vkBindImageMemory" => function!(bind, PFN_vkBindImageMemory),
        b"vkCreateFence" => function!(create_fence, PFN_vkCreateFence),
        b"vkDestroyFence" => function!(destroy_fence, PFN_vkDestroyFence),
        b"vkResetFences" => function!(reset_fences, PFN_vkResetFences),
        b"vkWaitForFences" => function!(wait_fences, PFN_vkWaitForFences),
        b"vkCmdPipelineBarrier" => function!(ignore_pipeline_barrier, PFN_vkCmdPipelineBarrier),
        b"vkCmdCopyImage" => function!(copy, PFN_vkCmdCopyImage),
        b"vkDeviceWaitIdle" => function!(idle, PFN_vkDeviceWaitIdle),
        _ => None,
    }
}
