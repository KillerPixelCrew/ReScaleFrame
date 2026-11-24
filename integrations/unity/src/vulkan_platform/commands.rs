// SPDX-License-Identifier: MIT
use super::{DeviceHandles, Dispatch, SUCCESS, VULKAN, borrowed};
use crate::package_state::rsf_unity_package_sr_recording;
use crate::vulkan_state::{self, CommandSlot};
use ash::vk::{self, Handle as _};
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::ptr::null;
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

pub(crate) unsafe fn rsf_uvk_gpu_begin(
    shared: &DeviceHandles,
    slot: &CommandSlot,
    ring: u32,
) -> bool {
    let dispatch = unsafe { shared.dispatch() };
    let fence = vk::Fence::from_raw(slot.complete);
    if unsafe { (dispatch.wait_fences)(shared.vk_device(), 1, &fence, vk::TRUE, 10_000_000_000) }
        != SUCCESS
    {
        return false;
    }
    let info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    for command in slot.vk.map(vk::CommandBuffer::from_raw) {
        if unsafe { (dispatch.reset_buffer)(command, vk::CommandBufferResetFlags::empty()) }
            != SUCCESS
            || unsafe { (dispatch.begin_buffer)(command, &info) } != SUCCESS
        {
            return false;
        }
    }
    let allocator = unsafe { borrowed::<ID3D12CommandAllocator>(&slot.allocator) };
    let commands = unsafe { borrowed::<ID3D12GraphicsCommandList>(&slot.commands) };
    if unsafe {
        allocator
            .Reset()
            .and_then(|()| commands.Reset(allocator, None::<&ID3D12PipelineState>))
    }
    .is_err()
    {
        return false;
    }
    let Some((epoch, serial)) = vulkan_state::begin_recording(ring) else {
        return false;
    };
    rsf_unity_package_sr_recording(slot.commands, epoch, ring, serial, serial - 1);
    true
}

pub(crate) unsafe fn rsf_uvk_gpu_upload(
    shared: &DeviceHandles,
    slot: &CommandSlot,
    waits: *const vk::Semaphore,
    count: u32,
) -> bool {
    let dispatch = unsafe { shared.dispatch() };
    let command = vk::CommandBuffer::from_raw(slot.vk[0]);
    if unsafe { (dispatch.end_buffer)(command) } != SUCCESS {
        return false;
    }
    let waits = if count == 0 {
        &[]
    } else if waits.is_null() {
        return false;
    } else {
        // The intercepted Present lends its wait list through this submission.
        unsafe { std::slice::from_raw_parts(waits, count as usize) }
    };
    let mut stages = Vec::new();
    let mut values = Vec::new();
    if stages.try_reserve_exact(waits.len()).is_err()
        || values.try_reserve_exact(waits.len()).is_err()
    {
        return false;
    }
    stages.resize(waits.len(), vk::PipelineStageFlags::TRANSFER);
    values.resize(waits.len(), 0);
    let Some(value) = VULKAN.lock().next_completion() else {
        return false;
    };
    let commands = [command];
    let signal = [vk::Semaphore::from_raw(shared.vk_semaphore)];
    let mut fence_info = vk::D3D12FenceSubmitInfoKHR::default()
        .wait_semaphore_values(&values)
        .signal_semaphore_values(std::slice::from_ref(&value));
    let info = vk::SubmitInfo::default()
        .push_next(&mut fence_info)
        .wait_semaphores(waits)
        .wait_dst_stage_mask(&stages)
        .command_buffers(&commands)
        .signal_semaphores(&signal);
    if unsafe { (dispatch.queue_submit)(shared.vk_queue(), 1, &info, vk::Fence::null()) } != SUCCESS
    {
        return false;
    }
    vulkan_state::upload_submitted();
    if VULKAN.lock().completion() != Some(value) {
        return false;
    }
    let queue = unsafe { borrowed::<ID3D12CommandQueue>(&shared.queue) };
    let fence = unsafe { borrowed::<ID3D12Fence>(&shared.fence) };
    unsafe { queue.Wait(fence, value).is_ok() }
}

pub(crate) unsafe fn rsf_uvk_gpu_submit(
    shared: &DeviceHandles,
    slot: &CommandSlot,
    ring: u32,
    ready: u64,
) -> bool {
    let dispatch = unsafe { shared.dispatch() };
    let command = vk::CommandBuffer::from_raw(slot.vk[1]);
    let list = unsafe { borrowed::<ID3D12GraphicsCommandList>(&slot.commands) };
    if unsafe { list.Close() }.is_err() || unsafe { (dispatch.end_buffer)(command) } != SUCCESS {
        return false;
    }
    let Some(value) = VULKAN.lock().submit(ring as usize) else {
        return false;
    };
    let queue = unsafe { borrowed::<ID3D12CommandQueue>(&shared.queue) };
    let fence = unsafe { borrowed::<ID3D12Fence>(&shared.fence) };
    let list = ManuallyDrop::new(Some(unsafe { ID3D12CommandList::from_raw(slot.commands) }));
    unsafe { queue.ExecuteCommandLists(std::slice::from_ref(&*list)) };
    if unsafe { queue.Signal(fence, value) }.is_err() {
        return false;
    }

    let waits = [vk::Semaphore::from_raw(shared.vk_semaphore)];
    let signals = [vk::Semaphore::from_raw(ready)];
    let count = usize::from(ready != 0);
    let mut fence_info = vk::D3D12FenceSubmitInfoKHR::default()
        .wait_semaphore_values(std::slice::from_ref(&value))
        .signal_semaphore_values(&[0][..count]);
    let commands = [command];
    let info = vk::SubmitInfo::default()
        .push_next(&mut fence_info)
        .wait_semaphores(&waits)
        .wait_dst_stage_mask(&[vk::PipelineStageFlags::TRANSFER])
        .command_buffers(&commands)
        .signal_semaphores(&signals[..count]);
    let complete = vk::Fence::from_raw(slot.complete);
    if unsafe { (dispatch.reset_fences)(shared.vk_device(), 1, &complete) } != SUCCESS
        || unsafe { (dispatch.queue_submit)(shared.vk_queue(), 1, &info, complete) } != SUCCESS
    {
        return false;
    }
    vulkan_state::handback_fenced();
    true
}

pub(crate) unsafe fn rsf_uvk_gpu_dx_barrier(
    list: *mut c_void,
    resource: *mut c_void,
    before: i32,
    after: i32,
) {
    let barrier = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: ManuallyDrop::new(Some(unsafe { ID3D12Resource::from_raw(resource) })),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: D3D12_RESOURCE_STATES(before),
                StateAfter: D3D12_RESOURCE_STATES(after),
            }),
        },
        ..Default::default()
    };
    unsafe { borrowed::<ID3D12GraphicsCommandList>(&list).ResourceBarrier(&[barrier]) };
}

pub(crate) unsafe fn rsf_uvk_gpu_dx_copy(
    list: *mut c_void,
    target: *mut c_void,
    source: *mut c_void,
) {
    unsafe {
        borrowed::<ID3D12GraphicsCommandList>(&list).CopyResource(
            borrowed::<ID3D12Resource>(&target),
            borrowed::<ID3D12Resource>(&source),
        )
    };
}

const COLOR: vk::ImageSubresourceLayers = vk::ImageSubresourceLayers {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    mip_level: 0,
    base_array_layer: 0,
    layer_count: 1,
};

unsafe fn image_barrier(dispatch: &Dispatch, command: u64, barrier: &vk::ImageMemoryBarrier<'_>) {
    unsafe {
        (dispatch.pipeline_barrier)(
            vk::CommandBuffer::from_raw(command),
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            0,
            null(),
            0,
            null(),
            1,
            barrier,
        )
    }
}

pub(crate) unsafe fn rsf_uvk_gpu_image_transition(
    shared: &DeviceHandles,
    command: u64,
    image: u64,
    index: u32,
    output: bool,
    acquire: bool,
) {
    let (transfer_layout, transfer_access) = if output {
        (
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            vk::AccessFlags::TRANSFER_READ,
        )
    } else {
        (
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::AccessFlags::TRANSFER_WRITE,
        )
    };
    let dispatch = unsafe { shared.dispatch() };
    let barrier = vk::ImageMemoryBarrier::default()
        .image(vk::Image::from_raw(image))
        .subresource_range(color_range());
    let barrier = if acquire {
        let old = if vulkan_state::record_image(index) {
            vk::ImageLayout::GENERAL
        } else {
            vk::ImageLayout::UNDEFINED
        };
        barrier
            .old_layout(old)
            .new_layout(transfer_layout)
            .dst_access_mask(transfer_access)
            .src_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
            .dst_queue_family_index(shared.vk_queue_family)
    } else {
        barrier
            .old_layout(transfer_layout)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(transfer_access)
            .src_queue_family_index(shared.vk_queue_family)
            .dst_queue_family_index(vk::QUEUE_FAMILY_EXTERNAL)
    };
    unsafe { image_barrier(dispatch, command, &barrier) };
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

pub(crate) unsafe fn rsf_uvk_gpu_copy(
    shared: &DeviceHandles,
    command: u64,
    source: u64,
    source_layout: i32,
    aspect: u32,
    target: u64,
    target_layout: i32,
    width: u32,
    height: u32,
) {
    let region = vk::ImageCopy::default()
        .src_subresource(COLOR.aspect_mask(vk::ImageAspectFlags::from_raw(aspect)))
        .dst_subresource(COLOR)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        });
    unsafe {
        (shared.dispatch().copy_image)(
            vk::CommandBuffer::from_raw(command),
            vk::Image::from_raw(source),
            vk::ImageLayout::from_raw(source_layout),
            vk::Image::from_raw(target),
            vk::ImageLayout::from_raw(target_layout),
            1,
            &region,
        )
    }
}

pub(crate) unsafe fn rsf_uvk_gpu_blit_present(
    shared: &DeviceHandles,
    command: u64,
    presented: u64,
    target: u64,
    width: u32,
    height: u32,
) {
    let dispatch = unsafe { shared.dispatch() };
    let presented = vk::Image::from_raw(presented);
    let barrier = vk::ImageMemoryBarrier::default()
        .image(presented)
        .subresource_range(color_range())
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
    let transfer = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
    let present = vk::ImageLayout::PRESENT_SRC_KHR;
    let read = vk::AccessFlags::TRANSFER_READ;
    unsafe {
        image_barrier(
            dispatch,
            command,
            &barrier
                .old_layout(present)
                .new_layout(transfer)
                .dst_access_mask(read),
        )
    };
    let offsets = [
        vk::Offset3D::default(),
        vk::Offset3D {
            x: width as i32,
            y: height as i32,
            z: 1,
        },
    ];
    let blit = vk::ImageBlit::default()
        .src_subresource(COLOR)
        .dst_subresource(COLOR)
        .src_offsets(offsets)
        .dst_offsets(offsets);
    unsafe {
        (dispatch.blit_image)(
            vk::CommandBuffer::from_raw(command),
            presented,
            transfer,
            vk::Image::from_raw(target),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            1,
            &blit,
            vk::Filter::NEAREST,
        );
        image_barrier(
            dispatch,
            command,
            &barrier
                .old_layout(transfer)
                .new_layout(present)
                .src_access_mask(read),
        );
    }
}
