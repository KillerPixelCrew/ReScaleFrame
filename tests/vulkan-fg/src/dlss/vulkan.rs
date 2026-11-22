// SPDX-License-Identifier: MIT
//! Ash doubles for the native DLSS-G and capture fixture.
use super::{COLOR, DST, Handle, handle};
use crate::sys::*;
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record;
use std::{
    cell::{Cell, UnsafeCell},
    ffi::{CStr, c_char},
    mem, slice,
};

const SUCCESS: vk::Result = vk::Result::SUCCESS;
const TIMEOUT_NS: u64 = 15_000_000_000;

thread_local! {
    static STATE: UnsafeCell<rsf_dlss_fixture_state> = UnsafeCell::new(rsf_dlss_fixture_state {
        lease_session: 7, lease_frame: 42, max_generated: 3, backbuffer_count: 3,
        surface_usage: (COLOR | DST).as_raw(), expected_chain_usage: (COLOR | DST).as_raw(),
        expected_inputs_value: 9, completion_value: 9, ..Default::default()
    });
    static CAPTURE: UnsafeCell<rsf_dlss_fixture_capture_state> = UnsafeCell::new(rsf_dlss_fixture_capture_state {
        pool_family: u32::MAX, ..Default::default()
    });
    static CALLBACK: Cell<rsf_sl_present_capture_callback> = const { Cell::new(None) };
    static READBACKS: Cell<u64> = const { Cell::new(0) };
    static NEXT: Cell<u64> = const { Cell::new(1000) };
    static TIMEOUT_EPOCH: Cell<u64> = const { Cell::new(0) };
    static LOCKED: Cell<bool> = const { Cell::new(false) };
    static IN_WSI: Cell<bool> = const { Cell::new(false) };
}

fn state<R>(update: impl FnOnce(&mut rsf_dlss_fixture_state) -> R) -> R {
    STATE.with(|state| unsafe { update(&mut *state.get()) })
}

fn capture<R>(update: impl FnOnce(&mut rsf_dlss_fixture_capture_state) -> R) -> R {
    CAPTURE.with(|state| unsafe { update(&mut *state.get()) })
}

fn next() -> u64 {
    let value = NEXT.get() + 1;
    NEXT.set(value);
    value
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_state() -> *mut rsf_dlss_fixture_state {
    STATE.with(UnsafeCell::get)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_sl_worker_timeout_epoch() -> u64 {
    TIMEOUT_EPOCH.get()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_locked() -> u32 {
    u32::from(LOCKED.get())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_lock(_: Handle) {
    assert!(!LOCKED.replace(true), "broker lock is never nested");
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_unlock(_: Handle) {
    assert!(LOCKED.replace(false), "broker unlock follows lock");
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_submit(
    user: Handle,
    count: u32,
    commands: *const core::ffi::c_void,
    fence: u64,
) -> i32 {
    assert!(
        user == handle(2) && count == 0 && commands.is_null() && fence != 0,
        "completion fences follow host graphics submissions"
    );
    state(|state| {
        state.fence_submits += 1;
        let result = if state.refuse_submit {
            vk::Result::ERROR_DEVICE_LOST
        } else {
            SUCCESS
        };
        result.as_raw()
    })
}

unsafe extern "system" fn create_images(
    device: vk::Device,
    info: *const vk::SwapchainCreateInfoKHR<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::SwapchainKHR,
) -> vk::Result {
    state(|state| {
        assert_eq!(
            (device.as_raw(), allocator.addr(), LOCKED.get(), unsafe {
                (*info).image_usage.as_raw()
            }),
            (1, 4, false, state.expected_chain_usage)
        );
        state.chain_creates += 1;
        unsafe { out.write(vk::SwapchainKHR::from_raw(3)) };
        if state.refuse_create {
            vk::Result::ERROR_INITIALIZATION_FAILED
        } else {
            SUCCESS
        }
    })
}

unsafe extern "system" fn destroy_images(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    allocator: *const vk::AllocationCallbacks<'_>,
) {
    assert_eq!(
        (
            device.as_raw(),
            chain.as_raw(),
            allocator.addr(),
            LOCKED.get()
        ),
        (1, 3, 4, false)
    );
    state(|state| state.destroyed_chains += 1);
}

unsafe extern "system" fn enumerate_images(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    images: *mut vk::Image,
) -> vk::Result {
    assert_eq!((device.as_raw(), chain.as_raw()), (1, 3));
    let (refused, available) = state(|state| (state.refuse_enumeration, state.backbuffer_count));
    if refused {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }
    if images.is_null() {
        unsafe { count.write(available) };
        return SUCCESS;
    }
    let capacity = unsafe { count.read() };
    if capacity > 3 {
        unsafe { count.write(3) };
        return vk::Result::INCOMPLETE;
    }
    for index in 0..capacity {
        unsafe {
            images
                .add(index as usize)
                .write(vk::Image::from_raw(u64::from(100 + index)))
        };
    }
    if capacity < 3 {
        vk::Result::INCOMPLETE
    } else {
        SUCCESS
    }
}

unsafe extern "system" fn acquire_image(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    timeout: u64,
    semaphore: vk::Semaphore,
    fence: vk::Fence,
    index: *mut u32,
) -> vk::Result {
    assert_eq!(
        (
            device.as_raw(),
            chain.as_raw(),
            timeout,
            semaphore.as_raw(),
            fence.as_raw()
        ),
        (1, 3, 8, 9, 10)
    );
    unsafe { index.write(2) };
    vk::Result::SUBOPTIMAL_KHR
}

unsafe extern "system" fn publish(
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    assert_eq!(
        (
            queue.as_raw(),
            unsafe { (*info).p_swapchains.read().as_raw() },
            LOCKED.get()
        ),
        (2, 3, false)
    );
    state(|state| state.present_calls += 1);
    SUCCESS
}

unsafe extern "system" fn idle(device: vk::Device) -> vk::Result {
    assert_eq!((device.as_raw(), LOCKED.get()), (1, true));
    if state(|state| state.worker_timeout) {
        TIMEOUT_EPOCH.set(TIMEOUT_EPOCH.get() + 1);
    }
    SUCCESS
}

unsafe extern "system" fn wait_semaphores(
    _: vk::Device,
    info: *const vk::SemaphoreWaitInfo<'_>,
    timeout: u64,
) -> vk::Result {
    let info = unsafe { &*info };
    assert_eq!(info.semaphore_count, 1);
    let semaphore = unsafe { info.p_semaphores.read() };
    let value = unsafe { info.p_values.read() };
    state(|state| {
        assert_eq!(
            (semaphore.as_raw(), value, timeout),
            (0xc9, state.expected_inputs_value, TIMEOUT_NS)
        );
        state.inputs_waits += 1;
        if state.refuse_inputs_wait {
            vk::Result::TIMEOUT
        } else {
            SUCCESS
        }
    })
}

unsafe extern "system" fn create_fence(
    _: vk::Device,
    _: *const vk::FenceCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Fence,
) -> vk::Result {
    unsafe { out.write(vk::Fence::from_raw(next())) };
    SUCCESS
}
unsafe extern "system" fn reset_fences(_: vk::Device, _: u32, _: *const vk::Fence) -> vk::Result {
    SUCCESS
}
unsafe extern "system" fn wait_fences(
    _: vk::Device,
    _: u32,
    fences: *const vk::Fence,
    _: vk::Bool32,
    timeout: u64,
) -> vk::Result {
    capture(|state| {
        state.waited_fence = unsafe { fences.read() }.as_raw() as usize;
        state.timeout = timeout;
    });
    SUCCESS
}
unsafe extern "system" fn destroy_fence(
    _: vk::Device,
    fence: vk::Fence,
    _: *const vk::AllocationCallbacks<'_>,
) {
    capture(|state| state.destroyed_fence = fence.as_raw() as usize);
}
unsafe extern "system" fn create_image(
    _: vk::Device,
    _: *const vk::ImageCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Image,
) -> vk::Result {
    unsafe { out.write(vk::Image::from_raw(next())) };
    SUCCESS
}
unsafe extern "system" fn destroy_image(
    _: vk::Device,
    _: vk::Image,
    _: *const vk::AllocationCallbacks<'_>,
) {
}
unsafe extern "system" fn image_requirements(
    _: vk::Device,
    _: vk::Image,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 4096,
            alignment: 8,
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
    SUCCESS
}
unsafe extern "system" fn free_memory(
    _: vk::Device,
    _: vk::DeviceMemory,
    _: *const vk::AllocationCallbacks<'_>,
) {
}
unsafe extern "system" fn bind_image(
    _: vk::Device,
    _: vk::Image,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    SUCCESS
}
unsafe extern "system" fn create_view(
    _: vk::Device,
    _: *const vk::ImageViewCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::ImageView,
) -> vk::Result {
    unsafe { out.write(vk::ImageView::from_raw(next())) };
    SUCCESS
}
unsafe extern "system" fn destroy_view(
    _: vk::Device,
    _: vk::ImageView,
    _: *const vk::AllocationCallbacks<'_>,
) {
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
unsafe extern "system" fn copy_image(
    _: vk::CommandBuffer,
    source: vk::Image,
    _: vk::ImageLayout,
    destination: vk::Image,
    _: vk::ImageLayout,
    _: u32,
    _: *const vk::ImageCopy,
) {
    assert!(
        source.as_raw() < 1000 && destination.as_raw() >= 1000,
        "copies read the leased engine image"
    );
    state(|state| state.copies += 1);
}
unsafe extern "system" fn memory_properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    let mut memory = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    memory.memory_types[0].property_flags = vk::MemoryPropertyFlags::DEVICE_LOCAL;
    unsafe { out.write(memory) };
}
unsafe extern "system" fn surface_capabilities(
    _: vk::PhysicalDevice,
    _: vk::SurfaceKHR,
    out: *mut vk::SurfaceCapabilitiesKHR,
) -> vk::Result {
    let usage = state(|state| state.surface_usage);
    unsafe {
        out.write(vk::SurfaceCapabilitiesKHR {
            supported_usage_flags: vk::ImageUsageFlags::from_raw(usage),
            ..Default::default()
        })
    };
    SUCCESS
}

unsafe extern "system" fn create_pool(
    device: vk::Device,
    info: *const vk::CommandPoolCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::CommandPool,
) -> vk::Result {
    assert_eq!(device.as_raw(), 1);
    let info = unsafe { &*info };
    capture(|state| {
        state.pool_family = info.queue_family_index;
        state.pool_flags = info.flags.as_raw();
    });
    unsafe { out.write(vk::CommandPool::from_raw(0x500)) };
    SUCCESS
}
unsafe extern "system" fn destroy_pool(
    _: vk::Device,
    pool: vk::CommandPool,
    _: *const vk::AllocationCallbacks<'_>,
) {
    assert_eq!(pool.as_raw(), 0x500);
    capture(|state| state.destroyed_pools += 1);
}
unsafe extern "system" fn reset_pool(
    _: vk::Device,
    pool: vk::CommandPool,
    _: vk::CommandPoolResetFlags,
) -> vk::Result {
    assert_eq!(pool.as_raw(), 0x500);
    SUCCESS
}
unsafe extern "system" fn allocate_commands(
    _: vk::Device,
    info: *const vk::CommandBufferAllocateInfo<'_>,
    out: *mut vk::CommandBuffer,
) -> vk::Result {
    let info = unsafe { &*info };
    assert_eq!(
        (info.command_pool.as_raw(), info.command_buffer_count),
        (0x500, 1)
    );
    assert!(info.level == vk::CommandBufferLevel::PRIMARY);
    unsafe { out.write(vk::CommandBuffer::from_raw(0x501)) };
    SUCCESS
}
unsafe extern "system" fn begin_commands(
    commands: vk::CommandBuffer,
    info: *const vk::CommandBufferBeginInfo<'_>,
) -> vk::Result {
    assert_eq!(commands.as_raw(), 0x501);
    assert!(unsafe { (*info).flags == vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT });
    capture(|state| state.begins += 1);
    SUCCESS
}
unsafe extern "system" fn end_commands(commands: vk::CommandBuffer) -> vk::Result {
    assert_eq!(commands.as_raw(), 0x501);
    capture(|state| state.ends += 1);
    SUCCESS
}

unsafe extern "C" fn wsi_lease(call: unsafe extern "C" fn(Handle) -> i32, user: Handle) -> i32 {
    assert!(!IN_WSI.replace(true), "WSI lease is never nested");
    capture(|state| state.wsi_calls += 1);
    let result = unsafe { call(user) };
    IN_WSI.set(false);
    result
}
unsafe extern "system" fn real_present(
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    assert!(IN_WSI.get() && queue.as_raw() == 2);
    capture(|state| {
        let result = unsafe { &*info };
        state.present_info = info.addr();
        state.present_waits = result.p_wait_semaphores.addr();
        state.present_extension = result.p_next.addr();
        state.present_chains = result.p_swapchains.addr();
        state.present_indices = result.p_image_indices.addr();
        state.present_wait_count = result.wait_semaphore_count;
        state.present_chain_count = result.swapchain_count;
        state.real_presents += 1;
    });
    vk::Result::SUBOPTIMAL_KHR
}
unsafe extern "system" fn real_images(
    device: vk::Device,
    chain: vk::SwapchainKHR,
    count: *mut u32,
    images: *mut vk::Image,
) -> vk::Result {
    assert!(IN_WSI.get() && device.as_raw() == 1 && chain.as_raw() == 0x33);
    capture(|state| state.real_enumerations += 1);
    if images.is_null() {
        unsafe { count.write(3) };
    } else {
        for index in 0..unsafe { count.read() } {
            unsafe {
                images
                    .add(index as usize)
                    .write(vk::Image::from_raw(u64::from(0x700 + index)))
            };
        }
    }
    SUCCESS
}
unsafe extern "system" fn real_submit(
    queue: vk::Queue,
    count: u32,
    info: *const vk::SubmitInfo<'_>,
    fence: vk::Fence,
) -> vk::Result {
    assert!(!IN_WSI.get() && queue.as_raw() == 2 && count == 1);
    let info = unsafe { &*info };
    assert!(info.wait_semaphore_count <= 2);
    capture(|state| {
        state.waits = info.p_wait_semaphores.addr();
        state.command_count = info.command_buffer_count;
        state.wait_count = info.wait_semaphore_count;
        state.submitted_fence = fence.as_raw() as usize;
        state.commands = unsafe { info.p_command_buffers.read() }.as_raw() as usize;
        state.real_submits += 1;
        for (index, stage) in state
            .stages
            .iter_mut()
            .enumerate()
            .take(info.wait_semaphore_count as usize)
        {
            *stage = unsafe { info.p_wait_dst_stage_mask.add(index).read() }.as_raw();
        }
    });
    SUCCESS
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_capture_calls_init(out: &mut rsf_dlss_fixture_capture_calls) {
    *out = rsf_dlss_fixture_capture_calls {
        present: real_present as *const () as usize,
        images: real_images as *const () as usize,
        submit: real_submit as *const () as usize,
        wsi: wsi_lease as *const () as usize,
    };
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_capture_state(out: &mut rsf_dlss_fixture_capture_state) {
    capture(|state| *out = *state);
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_reset_waited_fence() {
    capture(|state| state.waited_fence = 0);
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_dlss_capture_call(
    packet: *const rsf_sl_present_capture_packet,
) -> i32 {
    let user = capture(|state| state.user);
    unsafe { CALLBACK.get().unwrap()(user, packet) }
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_register_capture(
    value: &rsf_sl_present_capture_registration,
) -> i32 {
    assert_eq!(value.struct_size as usize, size_of_val(value));
    assert_eq!(value.abi_version, RSF_SL_PRESENT_CAPTURE_ABI_VERSION);
    capture(|state| {
        assert!(
            value.device == handle(1)
                && !value.user.is_null()
                && (state.user.is_null() || state.user == value.user)
        );
        state.user = value.user;
        state.registered = u32::from(value.callback.is_some());
        state.registrations += 1;
    });
    CALLBACK.set(value.callback);
    SUCCESS.as_raw()
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_dlss_record(
    _: Handle,
    commands: Handle,
    image: &rsf_srvk_image,
    record: &rsf_frame_record,
    role: *const c_char,
) -> u32 {
    assert_eq!(commands, handle(0x501));
    assert_eq!(image.device, handle(1));
    assert_eq!(
        (
            image.image,
            image.format,
            image.width,
            image.height,
            image.layout,
            record.frame_id
        ),
        (
            0x701,
            vk::Format::B8G8R8A8_UNORM.as_raw() as u32,
            1280,
            720,
            vk::ImageLayout::PRESENT_SRC_KHR.as_raw() as u32,
            42
        )
    );
    assert_ne!(image.copy_source, 0);
    assert_eq!(unsafe { CStr::from_ptr(role) }, c"fg-generated-present-2");
    READBACKS.set(READBACKS.get() + 1);
    1
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_readbacks(_: Handle) -> u64 {
    READBACKS.get()
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_flush(_: Handle) -> u32 {
    capture(|state| state.flushes += 1);
    1
}
#[unsafe(no_mangle)]
extern "C" fn rsf_fixture_dlss_failure(_: Handle, _: u64, reason: *const c_char, result: i32) {
    capture(|state| {
        state.failure = reason;
        state.failure_result = result;
    });
}
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_dlss_append(
    _: Handle,
    file: *const c_char,
    bytes: *const u8,
    length: usize,
) {
    if unsafe { CStr::from_ptr(file) } != c"dlss-present-capture.jsonl" {
        return;
    }
    let line = unsafe { slice::from_raw_parts(bytes, length) };
    let result = format!(
        "\"present_result\":{},",
        vk::Result::SUBOPTIMAL_KHR.as_raw()
    );
    for expected in [
        b"\"frame\":42,".as_slice(),
        b"\"sequence\":1,",
        result.as_bytes(),
        b"\"legacy_flush_attempted\":true,\"legacy_flush_succeeded\":true,",
        b"\"readback_completion_proof\":\"dedicated-capture-fences\"",
    ] {
        assert!(line.windows(expected.len()).any(|part| part == expected));
    }
    capture(|state| state.metadata += 1);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fixture_dlss_proc(_: Handle, name: *const c_char) -> rsf_vk_function {
    let name = unsafe { CStr::from_ptr(name) };
    macro_rules! entries {
        ($($name:literal, $ty:ident, $call:ident);+ $(;)?) => {
            $(if name == $name {
                return Some(unsafe { mem::transmute::<vk::$ty, unsafe extern "C" fn()>($call) });
            })+
        };
    }
    entries! {
        c"vkCreateCommandPool", PFN_vkCreateCommandPool, create_pool;
        c"vkDestroyCommandPool", PFN_vkDestroyCommandPool, destroy_pool;
        c"vkResetCommandPool", PFN_vkResetCommandPool, reset_pool;
        c"vkAllocateCommandBuffers", PFN_vkAllocateCommandBuffers, allocate_commands;
        c"vkBeginCommandBuffer", PFN_vkBeginCommandBuffer, begin_commands;
        c"vkEndCommandBuffer", PFN_vkEndCommandBuffer, end_commands;
        c"vkCreateSwapchainKHR", PFN_vkCreateSwapchainKHR, create_images;
        c"vkDestroySwapchainKHR", PFN_vkDestroySwapchainKHR, destroy_images;
        c"vkGetSwapchainImagesKHR", PFN_vkGetSwapchainImagesKHR, enumerate_images;
        c"vkAcquireNextImageKHR", PFN_vkAcquireNextImageKHR, acquire_image;
        c"vkQueuePresentKHR", PFN_vkQueuePresentKHR, publish;
        c"vkDeviceWaitIdle", PFN_vkDeviceWaitIdle, idle;
        c"vkWaitSemaphores", PFN_vkWaitSemaphores, wait_semaphores;
        c"vkCreateFence", PFN_vkCreateFence, create_fence;
        c"vkDestroyFence", PFN_vkDestroyFence, destroy_fence;
        c"vkResetFences", PFN_vkResetFences, reset_fences;
        c"vkWaitForFences", PFN_vkWaitForFences, wait_fences;
        c"vkCreateImage", PFN_vkCreateImage, create_image;
        c"vkDestroyImage", PFN_vkDestroyImage, destroy_image;
        c"vkGetImageMemoryRequirements", PFN_vkGetImageMemoryRequirements, image_requirements;
        c"vkAllocateMemory", PFN_vkAllocateMemory, allocate;
        c"vkFreeMemory", PFN_vkFreeMemory, free_memory;
        c"vkBindImageMemory", PFN_vkBindImageMemory, bind_image;
        c"vkCreateImageView", PFN_vkCreateImageView, create_view;
        c"vkDestroyImageView", PFN_vkDestroyImageView, destroy_view;
        c"vkCmdPipelineBarrier", PFN_vkCmdPipelineBarrier, barrier;
        c"vkCmdCopyImage", PFN_vkCmdCopyImage, copy_image;
        c"vkGetPhysicalDeviceMemoryProperties", PFN_vkGetPhysicalDeviceMemoryProperties, memory_properties;
        c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR", PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR, surface_capabilities;
    }
    None
}
