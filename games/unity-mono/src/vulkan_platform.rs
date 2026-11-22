// SPDX-License-Identifier: MIT
//! GPU operations for the Unity Vulkan bridge's leased shared-device and command records.

use crate::bridge_interop::VULKAN;
use crate::vulkan_state::{CommandSlot, DeviceHandles, ImageHandles};
use ash::vk::{self, Handle as _};
use rsf_runtime_submission::FenceStatus;
use std::alloc::{Layout, alloc};
use std::ffi::c_void;
use std::ptr::null;
use windows::Win32::{
    Foundation::{GENERIC_ALL, HANDLE, LUID},
    Graphics::{
        Direct3D::D3D_FEATURE_LEVEL_11_0,
        Direct3D12::*,
        Dxgi::{Common::*, CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory4},
    },
};
use windows::core::{Interface, PCWSTR};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, WaitForSingleObject},
};

mod commands;
mod surface;

const MEMORY_HANDLE: vk::ExternalMemoryHandleTypeFlags =
    vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE;
const FENCE_HANDLE: vk::ExternalSemaphoreHandleTypeFlags =
    vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE;
const SUCCESS: vk::Result = vk::Result::SUCCESS;

macro_rules! dispatch {
    ($($scope:ident $field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        struct Dispatch {
            $($field: vk::$ty,)+
        }
        impl Dispatch {
            unsafe fn load(
                instance: vk::Instance, device: vk::Device,
                gipa: vk::PFN_vkGetInstanceProcAddr, gdpa: vk::PFN_vkGetDeviceProcAddr,
            ) -> Option<Self> {
                let instance = |name: &std::ffi::CStr| unsafe { gipa(instance, name.as_ptr()) };
                let device = |name: &std::ffi::CStr| unsafe { gdpa(device, name.as_ptr()) };
                Some(Self {
                    // Each query uses the owner of the Vulkan operation and its exact signature.
                    $($field: unsafe { std::mem::transmute::<unsafe extern "system" fn(), vk::$ty>($scope($name)?) },)+
                })
            }
        }
    };
}

dispatch! {
    instance physical_properties: PFN_vkGetPhysicalDeviceProperties2 = c"vkGetPhysicalDeviceProperties2",
    instance memory_properties: PFN_vkGetPhysicalDeviceMemoryProperties = c"vkGetPhysicalDeviceMemoryProperties",
    device create_semaphore: PFN_vkCreateSemaphore = c"vkCreateSemaphore",
    device destroy_semaphore: PFN_vkDestroySemaphore = c"vkDestroySemaphore",
    device import_semaphore: PFN_vkImportSemaphoreWin32HandleKHR = c"vkImportSemaphoreWin32HandleKHR",
    device create_pool: PFN_vkCreateCommandPool = c"vkCreateCommandPool",
    device destroy_pool: PFN_vkDestroyCommandPool = c"vkDestroyCommandPool",
    device allocate_buffers: PFN_vkAllocateCommandBuffers = c"vkAllocateCommandBuffers",
    device create_fence: PFN_vkCreateFence = c"vkCreateFence",
    device destroy_fence: PFN_vkDestroyFence = c"vkDestroyFence",
    device fence_status: PFN_vkGetFenceStatus = c"vkGetFenceStatus",
    device wait_fences: PFN_vkWaitForFences = c"vkWaitForFences",
    device reset_fences: PFN_vkResetFences = c"vkResetFences",
    device reset_buffer: PFN_vkResetCommandBuffer = c"vkResetCommandBuffer",
    device begin_buffer: PFN_vkBeginCommandBuffer = c"vkBeginCommandBuffer",
    device end_buffer: PFN_vkEndCommandBuffer = c"vkEndCommandBuffer",
    device create_image: PFN_vkCreateImage = c"vkCreateImage",
    device destroy_image: PFN_vkDestroyImage = c"vkDestroyImage",
    device image_requirements: PFN_vkGetImageMemoryRequirements = c"vkGetImageMemoryRequirements",
    device allocate_memory: PFN_vkAllocateMemory = c"vkAllocateMemory",
    device free_memory: PFN_vkFreeMemory = c"vkFreeMemory",
    device bind_image: PFN_vkBindImageMemory = c"vkBindImageMemory",
    device pipeline_barrier: PFN_vkCmdPipelineBarrier = c"vkCmdPipelineBarrier",
    device copy_image: PFN_vkCmdCopyImage = c"vkCmdCopyImage",
    device blit_image: PFN_vkCmdBlitImage = c"vkCmdBlitImage",
    device queue_submit: PFN_vkQueueSubmit = c"vkQueueSubmit",
    device queue_idle: PFN_vkQueueWaitIdle = c"vkQueueWaitIdle",
}

impl DeviceHandles {
    fn vk_device(&self) -> vk::Device {
        vk::Device::from_raw(self.vk_device)
    }

    fn vk_queue(&self) -> vk::Queue {
        vk::Queue::from_raw(self.vk_queue)
    }

    unsafe fn dispatch(&self) -> &Dispatch {
        // Partial creation installs this before creating any Vulkan children.
        unsafe { &*self.dispatch.cast::<Dispatch>() }
    }
}

unsafe fn borrow<T: Interface>(raw: &*mut c_void) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.expect("live Unity Vulkan bridge COM view")
}

unsafe fn release<T: Interface>(raw: *mut c_void) {
    if !raw.is_null() {
        drop(unsafe { T::from_raw(raw) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_create_device(
    shared: *mut DeviceHandles,
    instance: u64,
    gipa: vk::PFN_vkGetInstanceProcAddr,
    gdpa: vk::PFN_vkGetDeviceProcAddr,
) -> u32 {
    let shared = unsafe { &mut *shared };
    let Some(dispatch) = (unsafe {
        Dispatch::load(
            vk::Instance::from_raw(instance),
            shared.vk_device(),
            gipa,
            gdpa,
        )
    }) else {
        return 0;
    };
    let mut id = vk::PhysicalDeviceIDProperties::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut id);
    unsafe {
        (dispatch.physical_properties)(
            vk::PhysicalDevice::from_raw(shared.vk_physical_device),
            &mut properties,
        )
    };
    if id.device_luid_valid == vk::FALSE {
        return 2;
    }
    let luid = LUID {
        LowPart: u32::from_ne_bytes(id.device_luid[..4].try_into().unwrap()),
        HighPart: i32::from_ne_bytes(id.device_luid[4..].try_into().unwrap()),
    };
    // The shared-device transaction can fail and retry even when allocation is refused.
    let owned = unsafe { alloc(Layout::new::<Dispatch>()).cast::<Dispatch>() };
    if owned.is_null() {
        return 0;
    }
    unsafe { owned.write(dispatch) };
    shared.dispatch = owned.cast();
    let created = unsafe { create_dx_device(luid, shared) };
    if !created {
        return 0;
    }
    let device = unsafe { borrow::<ID3D12Device>(&shared.device) };
    let fence = unsafe { borrow::<ID3D12Fence>(&shared.fence) };
    let Ok(handle) =
        (unsafe { device.CreateSharedHandle(fence, None, GENERIC_ALL.0, PCWSTR::null()) })
    else {
        return 0;
    };
    let dispatch = unsafe { shared.dispatch() };
    let mut semaphore = vk::Semaphore::null();
    let mut result = unsafe {
        (dispatch.create_semaphore)(
            shared.vk_device(),
            &vk::SemaphoreCreateInfo::default(),
            null(),
            &mut semaphore,
        )
    };
    if result == SUCCESS {
        shared.vk_semaphore = semaphore.as_raw();
        let info = vk::ImportSemaphoreWin32HandleInfoKHR::default()
            .semaphore(semaphore)
            .handle_type(FENCE_HANDLE)
            .handle(handle.0 as isize);
        result = unsafe { (dispatch.import_semaphore)(shared.vk_device(), &info) };
    }
    unsafe { CloseHandle(handle.0) };
    u32::from(result == SUCCESS)
}

unsafe fn create_dx_device(luid: LUID, shared: &mut DeviceHandles) -> bool {
    (|| -> windows::core::Result<bool> {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1()? };
        let adapter: IDXGIAdapter1 = unsafe { factory.EnumAdapterByLuid(luid)? };
        let mut device = None::<ID3D12Device>;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device)? };
        let Some(device) = device else {
            return Ok(false);
        };
        shared.device = device.into_raw();
        let device = unsafe { borrow::<ID3D12Device>(&shared.device) };
        let queue: ID3D12CommandQueue =
            unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default())? };
        shared.queue = queue.into_raw();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_SHARED)? };
        shared.fence = fence.into_raw();
        Ok(true)
    })()
    .unwrap_or(false)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_destroy_device(shared: *const DeviceHandles) {
    let shared = unsafe { &*shared };
    if !shared.dispatch.is_null() {
        let dispatch = unsafe { Box::from_raw(shared.dispatch.cast::<Dispatch>()) };
        if shared.vk_semaphore != 0 {
            unsafe {
                (dispatch.destroy_semaphore)(
                    shared.vk_device(),
                    vk::Semaphore::from_raw(shared.vk_semaphore),
                    null(),
                )
            };
        }
    }
    unsafe {
        release::<ID3D12Fence>(shared.fence);
        release::<ID3D12CommandQueue>(shared.queue);
        release::<ID3D12Device>(shared.device);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_create_image(
    shared: *const DeviceHandles,
    width: u32,
    height: u32,
    format: u32,
    dx_format: u32,
    output: *mut ImageHandles,
) -> bool {
    let shared = unsafe { &*shared };
    let output = unsafe { &mut *output };
    let device = unsafe { borrow::<ID3D12Device>(&shared.device) };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: u64::from(width),
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: DXGI_FORMAT(dx_format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS,
        ..Default::default()
    };
    let mut resource = None::<ID3D12Resource>;
    if unsafe {
        device.CreateCommittedResource(
            &heap,
            D3D12_HEAP_FLAG_SHARED,
            &desc,
            D3D12_RESOURCE_STATE_COMMON,
            None,
            &mut resource,
        )
    }
    .is_err()
    {
        return false;
    }
    let Some(resource) = resource else {
        return false;
    };
    output.resource = resource.into_raw();

    let dispatch = unsafe { shared.dispatch() };
    let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(MEMORY_HANDLE);
    let info = vk::ImageCreateInfo::default()
        .push_next(&mut external)
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::from_raw(format as i32))
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::TRANSFER_DST);
    let mut image = vk::Image::null();
    if unsafe { (dispatch.create_image)(shared.vk_device(), &info, null(), &mut image) } != SUCCESS
    {
        return false;
    }
    output.image = image.as_raw();

    let mut requirements = vk::MemoryRequirements::default();
    let mut properties = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        (dispatch.image_requirements)(shared.vk_device(), image, &mut requirements);
        (dispatch.memory_properties)(
            vk::PhysicalDevice::from_raw(shared.vk_physical_device),
            &mut properties,
        );
    }
    let Some(memory_type) = (0..properties.memory_type_count).find(|&index| {
        requirements.memory_type_bits & (1 << index) != 0
            && properties.memory_types[index as usize]
                .property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
    }) else {
        return false;
    };
    let resource = unsafe { borrow::<ID3D12Resource>(&output.resource) };
    let Ok(handle) =
        (unsafe { device.CreateSharedHandle(resource, None, GENERIC_ALL.0, PCWSTR::null()) })
    else {
        return false;
    };
    let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
    let mut import = vk::ImportMemoryWin32HandleInfoKHR::default()
        .handle_type(MEMORY_HANDLE)
        .handle(handle.0 as isize);
    let allocation = vk::MemoryAllocateInfo::default()
        .push_next(&mut dedicated)
        .push_next(&mut import)
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    let mut memory = vk::DeviceMemory::null();
    let result =
        unsafe { (dispatch.allocate_memory)(shared.vk_device(), &allocation, null(), &mut memory) };
    unsafe { CloseHandle(handle.0) };
    if result != SUCCESS {
        return false;
    }
    output.memory = memory.as_raw();
    unsafe { (dispatch.bind_image)(shared.vk_device(), image, memory, 0) == SUCCESS }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_destroy_image(
    shared: *const DeviceHandles,
    image: *const ImageHandles,
) {
    let (shared, image) = unsafe { (&*shared, &*image) };
    let dispatch = unsafe { shared.dispatch() };
    if image.image != 0 {
        unsafe {
            (dispatch.destroy_image)(shared.vk_device(), vk::Image::from_raw(image.image), null())
        };
    }
    if image.memory != 0 {
        unsafe {
            (dispatch.free_memory)(
                shared.vk_device(),
                vk::DeviceMemory::from_raw(image.memory),
                null(),
            )
        };
    }
    unsafe { release::<ID3D12Resource>(image.resource) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_create_pool(shared: *const DeviceHandles) -> u64 {
    let shared = unsafe { &*shared };
    let info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(shared.vk_queue_family)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    let mut pool = vk::CommandPool::null();
    let result =
        unsafe { (shared.dispatch().create_pool)(shared.vk_device(), &info, null(), &mut pool) };
    if result == SUCCESS { pool.as_raw() } else { 0 }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_destroy_pool(shared: *const DeviceHandles, pool: u64) {
    let shared = unsafe { &*shared };
    unsafe {
        (shared.dispatch().destroy_pool)(
            shared.vk_device(),
            vk::CommandPool::from_raw(pool),
            null(),
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_create_slot(
    shared: *const DeviceHandles,
    pool: u64,
    slot: *mut CommandSlot,
) -> bool {
    let (shared, slot) = unsafe { (&*shared, &mut *slot) };
    let dispatch = unsafe { shared.dispatch() };
    let info = vk::CommandBufferAllocateInfo::default()
        .command_pool(vk::CommandPool::from_raw(pool))
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(2);
    let mut buffers = [vk::CommandBuffer::null(); 2];
    if unsafe { (dispatch.allocate_buffers)(shared.vk_device(), &info, buffers.as_mut_ptr()) }
        != SUCCESS
    {
        return false;
    }
    slot.vk = buffers.map(|buffer| buffer.as_raw());
    let info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
    let mut fence = vk::Fence::null();
    if unsafe { (dispatch.create_fence)(shared.vk_device(), &info, null(), &mut fence) } != SUCCESS
    {
        return false;
    }
    slot.complete = fence.as_raw();
    let device = unsafe { borrow::<ID3D12Device>(&shared.device) };
    let Ok(allocator) = (unsafe {
        device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT)
    }) else {
        return false;
    };
    slot.allocator = allocator.into_raw();
    let allocator = unsafe { borrow::<ID3D12CommandAllocator>(&slot.allocator) };
    let result = unsafe {
        device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
            0,
            D3D12_COMMAND_LIST_TYPE_DIRECT,
            allocator,
            None::<&ID3D12PipelineState>,
        )
    };
    let Ok(commands) = result else { return false };
    slot.commands = commands.into_raw();
    unsafe {
        borrow::<ID3D12GraphicsCommandList>(&slot.commands)
            .Close()
            .is_ok()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_destroy_slot(
    shared: *const DeviceHandles,
    slot: *const CommandSlot,
) {
    let (shared, slot) = unsafe { (&*shared, &*slot) };
    if slot.complete != 0 {
        unsafe {
            (shared.dispatch().destroy_fence)(
                shared.vk_device(),
                vk::Fence::from_raw(slot.complete),
                null(),
            )
        };
    }
    unsafe {
        release::<ID3D12GraphicsCommandList>(slot.commands);
        release::<ID3D12CommandAllocator>(slot.allocator);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_fence_ready(shared: *const DeviceHandles, fence: u64) -> bool {
    let shared = unsafe { &*shared };
    unsafe {
        (shared.dispatch().fence_status)(shared.vk_device(), vk::Fence::from_raw(fence)) == SUCCESS
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_shared_completed(shared: *const DeviceHandles) -> u64 {
    unsafe { borrow::<ID3D12Fence>(&(*shared).fence).GetCompletedValue() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_uvk_gpu_drain(shared: *const DeviceHandles) -> bool {
    let shared = unsafe { &*shared };
    if unsafe { (shared.dispatch().queue_idle)(shared.vk_queue()) } != SUCCESS {
        return false;
    }
    let fence = unsafe { borrow::<ID3D12Fence>(&shared.fence) };
    let completed = unsafe { fence.GetCompletedValue() };
    let (status, pending) = {
        let ring = VULKAN.lock();
        (ring.status(completed), ring.submitted())
    };
    match status {
        FenceStatus::Complete => true,
        FenceStatus::Removed => false,
        FenceStatus::Pending => {
            let event = unsafe { CreateEventW(null(), 0, 0, null()) };
            if event.is_null() {
                return false;
            }
            let done = unsafe {
                fence.SetEventOnCompletion(pending, HANDLE(event)).is_ok()
                    && WaitForSingleObject(event, 10_000) == WAIT_OBJECT_0
            };
            unsafe { CloseHandle(event) };
            let completed = unsafe { fence.GetCompletedValue() };
            done && VULKAN.lock().drained(completed)
        }
    }
}
