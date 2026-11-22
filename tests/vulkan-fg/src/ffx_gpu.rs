// SPDX-License-Identifier: MIT
use crate::ffx_abi::{rsf_ffx_fixture_state, rsf_vk_function, rsf_wsi_route_fixture_state};
use ash::vk::{self, Handle};
use std::{
    cell::UnsafeCell,
    ffi::{CStr, c_char, c_void},
};

// Scenario, SDK callbacks and fake Vulkan calls all execute on the initiating
// thread. Use raw pointers because Present can synchronously reenter teardown.
thread_local! {
    static FIXTURE: UnsafeCell<rsf_ffx_fixture_state> = const { UnsafeCell::new(unsafe { std::mem::zeroed() }) };
    static ROUTE: UnsafeCell<rsf_wsi_route_fixture_state> = const { UnsafeCell::new(unsafe { std::mem::zeroed() }) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_ffx_state() -> *mut rsf_ffx_fixture_state {
    state()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_wsi_route_state() -> *mut rsf_wsi_route_fixture_state {
    route()
}

pub(crate) unsafe extern "system" fn rsf_fixture_ffx_images(
    _: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    images: *mut vk::Image,
) -> vk::Result {
    assert_eq!(chain.as_raw(), unsafe {
        crate::ffx_abi::rsf_fixture_ffx_chain()
    });
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

pub(crate) unsafe extern "system" fn rsf_fixture_ffx_acquire(
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

pub(crate) extern "C" fn rsf_fixture_ffx_present_count(_: vk::SwapchainKHR) -> u64 {
    u64::from(unsafe { (*state()).present_calls })
}

// The FFX scenarios and their fake queue calls run on one thread.
pub(super) fn state() -> *mut rsf_ffx_fixture_state {
    FIXTURE.with(UnsafeCell::get)
}

fn next() -> u64 {
    let state = state();
    unsafe {
        (*state).next += 1;
        (*state).next
    }
}

fn route() -> *mut rsf_wsi_route_fixture_state {
    ROUTE.with(UnsafeCell::get)
}

unsafe extern "system" fn wsi_present(
    queue: vk::Queue,
    _: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    use crate::ffx_abi::*;
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
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_wsi_entry(name: *const c_char) -> rsf_vk_function {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_wsi_lookup(
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

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_ffx_submit(
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

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_ffx_lock(_: *mut c_void) {
    unsafe { (*state()).locks += 1 };
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_ffx_unlock(_: *mut c_void) {
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
unsafe extern "system" fn barrier(
    _: vk::CommandBuffer,
    _: vk::PipelineStageFlags,
    _: vk::PipelineStageFlags,
    _: vk::DependencyFlags,
    _: u32,
    _: *const vk::MemoryBarrier<'_>,
    _: u32,
    _: *const vk::BufferMemoryBarrier<'_>,
    _: u32,
    _: *const vk::ImageMemoryBarrier<'_>,
) {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_ffx_lookup(
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
        b"vkCmdPipelineBarrier" => function!(barrier, PFN_vkCmdPipelineBarrier),
        b"vkCmdCopyImage" => function!(copy, PFN_vkCmdCopyImage),
        b"vkDeviceWaitIdle" => function!(idle, PFN_vkDeviceWaitIdle),
        _ => None,
    }
}
