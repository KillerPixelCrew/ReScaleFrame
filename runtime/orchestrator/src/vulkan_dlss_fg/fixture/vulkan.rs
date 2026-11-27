// SPDX-License-Identifier: MIT
//! Ash doubles for the native DLSS-G and capture fixture, and the fixture's observed state.
use super::{COLOR, DST, Handle, handle};
use crate::sys::{
    RSF_SL_PRESENT_CAPTURE_ABI_VERSION, rsf_sl_present_capture_callback as Callback,
    rsf_sl_present_capture_packet as Packet, rsf_sl_present_capture_registration as Registration,
    rsf_srvk_image as SourceImage, rsf_vk_function,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record;
use rsf_test_support::{ignore_pipeline_barrier, vk_entry};
use std::{
    cell::{Cell, UnsafeCell},
    ffi::{CStr, c_char},
    ptr, slice,
};

/// What the provider scenarios read and steer between calls into the provider.
#[derive(Default)]
pub(super) struct State {
    pub(super) lease_session: u64,
    pub(super) lease_frame: u64,
    pub(super) on_calls: u32,
    pub(super) off_calls: u32,
    pub(super) present_calls: u32,
    pub(super) fence_submits: u32,
    pub(super) destroyed_chains: u32,
    pub(super) sdk_presented: u32,
    pub(super) copies: u32,
    pub(super) release_calls: u32,
    pub(super) max_generated: u32,
    pub(super) backbuffer_count: u32,
    pub(super) surface_usage: u32,
    pub(super) expected_chain_usage: u32,
    pub(super) chain_creates: u32,
    pub(super) inputs_waits: u32,
    pub(super) refuse_create: bool,
    pub(super) refuse_enumeration: bool,
    pub(super) refuse_options: bool,
    pub(super) refuse_submit: bool,
    pub(super) refuse_tags: bool,
    pub(super) refuse_release: bool,
    pub(super) worker_timeout: bool,
    pub(super) refuse_inputs_wait: bool,
    pub(super) expected_inputs_value: u64,
    pub(super) completion_value: u64,
}

/// The real present, image, submit and WSI entries the capture packet carries.
#[derive(Default)]
pub(super) struct CaptureCalls {
    pub(super) present: usize,
    pub(super) images: usize,
    pub(super) submit: usize,
    pub(super) wsi: usize,
}

/// What the present capture owner did through its real Vulkan, WSI and registration leaves.
#[derive(Clone, Copy)]
pub(super) struct CaptureState {
    pub(super) user: Handle,
    pub(super) registered: u32,
    pub(super) registrations: u32,
    pub(super) pool_family: u32,
    pub(super) pool_flags: u32,
    pub(super) begins: u32,
    pub(super) ends: u32,
    pub(super) destroyed_pools: u32,
    pub(super) real_submits: u32,
    pub(super) real_enumerations: u32,
    pub(super) real_presents: u32,
    pub(super) wsi_calls: u32,
    pub(super) waits: usize,
    pub(super) commands: usize,
    pub(super) submitted_fence: usize,
    pub(super) waited_fence: usize,
    pub(super) destroyed_fence: usize,
    pub(super) wait_count: u32,
    pub(super) command_count: u32,
    pub(super) stages: [u32; 2],
    pub(super) timeout: u64,
    pub(super) flushes: u32,
    pub(super) metadata: u32,
    pub(super) present_info: usize,
    pub(super) present_waits: usize,
    pub(super) present_extension: usize,
    pub(super) present_chains: usize,
    pub(super) present_indices: usize,
    pub(super) present_wait_count: u32,
    pub(super) present_chain_count: u32,
    pub(super) failure: *const c_char,
    pub(super) failure_result: i32,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            user: ptr::null_mut(),
            registered: 0,
            registrations: 0,
            pool_family: 0,
            pool_flags: 0,
            begins: 0,
            ends: 0,
            destroyed_pools: 0,
            real_submits: 0,
            real_enumerations: 0,
            real_presents: 0,
            wsi_calls: 0,
            waits: 0,
            commands: 0,
            submitted_fence: 0,
            waited_fence: 0,
            destroyed_fence: 0,
            wait_count: 0,
            command_count: 0,
            stages: [0; 2],
            timeout: 0,
            flushes: 0,
            metadata: 0,
            present_info: 0,
            present_waits: 0,
            present_extension: 0,
            present_chains: 0,
            present_indices: 0,
            present_wait_count: 0,
            present_chain_count: 0,
            failure: ptr::null(),
            failure_result: 0,
        }
    }
}

const SUCCESS: vk::Result = vk::Result::SUCCESS;
const TIMEOUT_NS: u64 = 15_000_000_000;

thread_local! {
    static STATE: UnsafeCell<State> = UnsafeCell::new(State {
        lease_session: 7, lease_frame: 42, max_generated: 3, backbuffer_count: 3,
        surface_usage: (COLOR | DST).as_raw(), expected_chain_usage: (COLOR | DST).as_raw(),
        expected_inputs_value: 9, completion_value: 9, ..Default::default()
    });
    static CAPTURE: UnsafeCell<CaptureState> = UnsafeCell::new(CaptureState {
        pool_family: u32::MAX, ..Default::default()
    });
    static CALLBACK: Cell<Callback> = const { Cell::new(None) };
    static READBACKS: Cell<u64> = const { Cell::new(0) };
    static NEXT: Cell<u64> = const { Cell::new(1000) };
    static TIMEOUT_EPOCH: Cell<u64> = const { Cell::new(0) };
    static LOCKED: Cell<bool> = const { Cell::new(false) };
    static IN_WSI: Cell<bool> = const { Cell::new(false) };
}

fn state<R>(update: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|state| unsafe { update(&mut *state.get()) })
}

fn capture<R>(update: impl FnOnce(&mut CaptureState) -> R) -> R {
    CAPTURE.with(|state| unsafe { update(&mut *state.get()) })
}

fn next() -> u64 {
    let value = NEXT.get() + 1;
    NEXT.set(value);
    value
}

pub(super) fn state_ptr() -> *mut State {
    STATE.with(UnsafeCell::get)
}

/// The Streamline worker epoch of the synthetic tables; a worker timeout advances it.
pub(super) unsafe extern "C" fn timeout_epoch() -> u64 {
    TIMEOUT_EPOCH.get()
}

pub(super) fn locked() -> bool {
    LOCKED.get()
}

pub(super) unsafe extern "C" fn lock(_: Handle) {
    assert!(!LOCKED.replace(true), "broker lock is never nested");
}

pub(super) unsafe extern "C" fn unlock(_: Handle) {
    assert!(LOCKED.replace(false), "broker unlock follows lock");
}

pub(super) unsafe extern "C" fn submit(
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

pub(super) fn capture_calls() -> CaptureCalls {
    CaptureCalls {
        present: real_present as *const () as usize,
        images: real_images as *const () as usize,
        submit: real_submit as *const () as usize,
        wsi: wsi_lease as *const () as usize,
    }
}
pub(super) fn capture_state() -> CaptureState {
    capture(|state| *state)
}
pub(super) fn reset_waited_fence() {
    capture(|state| state.waited_fence = 0);
}
pub(super) unsafe fn capture_call(packet: *const Packet) -> i32 {
    let user = capture(|state| state.user);
    unsafe { CALLBACK.get().unwrap()(user, packet) }
}
pub(super) unsafe extern "C" fn register_capture(value: *const Registration) -> i32 {
    let value = unsafe { &*value };
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
pub(super) unsafe extern "C" fn record(
    _: Handle,
    commands: Handle,
    image: *const SourceImage,
    record: *const rsf_frame_record,
    role: *const c_char,
) -> u32 {
    let (image, record) = unsafe { (&*image, &*record) };
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
pub(super) unsafe extern "C" fn readbacks(_: Handle) -> u64 {
    READBACKS.get()
}
pub(super) unsafe extern "C" fn flush(_: Handle) -> u32 {
    capture(|state| state.flushes += 1);
    1
}
pub(super) unsafe extern "C" fn failure(_: Handle, _: u64, reason: *const c_char, result: i32) {
    capture(|state| {
        state.failure = reason;
        state.failure_result = result;
    });
}
pub(super) unsafe extern "C" fn append(
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

pub(super) unsafe extern "C" fn proc_address(_: Handle, name: *const c_char) -> rsf_vk_function {
    let name = unsafe { CStr::from_ptr(name) };
    macro_rules! entries {
        ($($name:literal, $ty:ident, $call:ident);+ $(;)?) => {
            $(if name == $name {
                return vk_entry::<vk::$ty>($call);
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
        c"vkCmdPipelineBarrier", PFN_vkCmdPipelineBarrier, ignore_pipeline_barrier;
        c"vkCmdCopyImage", PFN_vkCmdCopyImage, copy_image;
        c"vkGetPhysicalDeviceMemoryProperties", PFN_vkGetPhysicalDeviceMemoryProperties, memory_properties;
        c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR", PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR, surface_capabilities;
    }
    None
}
