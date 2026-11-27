// SPDX-License-Identifier: MIT
// The capture owner's Vulkan leaves. Unit tests call fakes instead, so the leaves are unused there.
#![cfg_attr(test, allow(dead_code))]
use super::{Allocation, Handle, Image};
use crate::dx12_vulkan_platform::entry;
use crate::sys::rsf_vk_get_proc;
use ash::vk::{self, Handle as _};
use std::ptr;

const _: () = assert!(size_of::<vk::PhysicalDeviceMemoryProperties>() == 520);

macro_rules! dispatch {
    ($($field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        pub(super) struct Dispatch {
            device: vk::Device,
            memory: vk::PhysicalDeviceMemoryProperties,
            events: Option<Events>,
            $($field: vk::$ty,)+
        }

        impl Dispatch {
            pub(super) fn load(device: Handle, proc: rsf_vk_get_proc, memory: vk::PhysicalDeviceMemoryProperties) -> Option<Self> {
                if device.is_null() {
                    return None;
                }
                // Each name is paired with its exact system-ABI signature.
                $(let $field = unsafe { entry::<vk::$ty>(proc, device, $name) }?;)+
                Some(Self {
                    device: vk::Device::from_raw(device as u64),
                    memory,
                    events: Events::load(device, proc),
                    $($field,)+
                })
            }
        }
    };
}

struct Events {
    create: vk::PFN_vkCreateEvent,
    destroy: vk::PFN_vkDestroyEvent,
    set: vk::PFN_vkCmdSetEvent,
    status: vk::PFN_vkGetEventStatus,
}

impl Events {
    fn load(device: Handle, proc: rsf_vk_get_proc) -> Option<Self> {
        // Each pointer comes from the supplied resolver for this device.
        unsafe {
            Some(Self {
                create: entry(proc, device, c"vkCreateEvent")?,
                destroy: entry(proc, device, c"vkDestroyEvent")?,
                set: entry(proc, device, c"vkCmdSetEvent")?,
                status: entry(proc, device, c"vkGetEventStatus")?,
            })
        }
    }
}

dispatch! {
    create_buffer: PFN_vkCreateBuffer = c"vkCreateBuffer",
    destroy_buffer: PFN_vkDestroyBuffer = c"vkDestroyBuffer",
    requirements: PFN_vkGetBufferMemoryRequirements = c"vkGetBufferMemoryRequirements",
    allocate: PFN_vkAllocateMemory = c"vkAllocateMemory",
    free: PFN_vkFreeMemory = c"vkFreeMemory",
    bind: PFN_vkBindBufferMemory = c"vkBindBufferMemory",
    map: PFN_vkMapMemory = c"vkMapMemory",
    unmap: PFN_vkUnmapMemory = c"vkUnmapMemory",
    barrier: PFN_vkCmdPipelineBarrier = c"vkCmdPipelineBarrier",
    copy: PFN_vkCmdCopyImageToBuffer = c"vkCmdCopyImageToBuffer",
}

// Creates the buffer, host-coherent memory and optional event. Partial handles stay in `out` on
// failure; the owner releases them later.
pub(super) unsafe fn allocate(
    native: Handle,
    _: *const Image,
    bytes: u64,
    event: u32,
    out: *mut Allocation,
) -> i32 {
    let (vk, out) = unsafe { (&*native.cast::<Dispatch>(), &mut *out) };
    let info = vk::BufferCreateInfo::default()
        .size(bytes)
        .usage(vk::BufferUsageFlags::TRANSFER_DST);
    let mut buffer = vk::Buffer::null();
    let result = unsafe { (vk.create_buffer)(vk.device, &info, ptr::null(), &mut buffer) };
    if result != vk::Result::SUCCESS {
        return result.as_raw();
    }
    out.buffer = buffer.as_raw();
    let mut required = vk::MemoryRequirements::default();
    unsafe { (vk.requirements)(vk.device, buffer, &mut required) };
    let flags = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let Some(index) = (0..vk.memory.memory_type_count.min(32)).find(|&i| {
        required.memory_type_bits & (1 << i) != 0
            && vk.memory.memory_types[i as usize]
                .property_flags
                .contains(flags)
    }) else {
        return vk::Result::ERROR_FEATURE_NOT_PRESENT.as_raw();
    };
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(required.size)
        .memory_type_index(index);
    let mut memory = vk::DeviceMemory::null();
    let result = unsafe { (vk.allocate)(vk.device, &info, ptr::null(), &mut memory) };
    if result != vk::Result::SUCCESS {
        return result.as_raw();
    }
    out.memory = memory.as_raw();
    let result = unsafe { (vk.bind)(vk.device, buffer, memory, 0) };
    if result != vk::Result::SUCCESS || event == 0 {
        return result.as_raw();
    }
    let Some(events) = &vk.events else {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    };
    let mut handle = vk::Event::null();
    let result = unsafe {
        (events.create)(
            vk.device,
            &vk::EventCreateInfo::default(),
            ptr::null(),
            &mut handle,
        )
    };
    if result == vk::Result::SUCCESS {
        out.event = handle.as_raw();
    }
    result.as_raw()
}

// Records the image-to-buffer copy, its barriers and the optional event.
pub(super) unsafe fn encode(
    native: Handle,
    commands: Handle,
    source: *const Image,
    allocation: *const Allocation,
) {
    let (vk, source, allocation) = unsafe { (&*native.cast::<Dispatch>(), &*source, &*allocation) };
    let commands = vk::CommandBuffer::from_raw(commands as u64);
    let all = vk::PipelineStageFlags::ALL_COMMANDS;
    let transfer = vk::PipelineStageFlags::TRANSFER;
    let memory = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
    let original = vk::ImageLayout::from_raw(source.layout as i32);
    let copy_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
    let aspect = if source.format == vk::Format::D32_SFLOAT.as_raw() as u32 {
        vk::ImageAspectFlags::DEPTH
    } else {
        vk::ImageAspectFlags::COLOR
    };
    let mut image = vk::ImageMemoryBarrier::default()
        .old_layout(original)
        .new_layout(copy_layout)
        .src_access_mask(memory)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(vk::Image::from_raw(source.image))
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(aspect)
                .level_count(1)
                .layer_count(1),
        );
    let region = vk::BufferImageCopy::default()
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(aspect)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width: source.width,
            height: source.height,
            depth: 1,
        });
    let buffer = vk::Buffer::from_raw(allocation.buffer);
    let host = vk::BufferMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::HOST_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .buffer(buffer)
        .size(vk::WHOLE_SIZE);
    let flags = vk::DependencyFlags::empty();
    unsafe {
        (vk.barrier)(
            commands,
            all,
            transfer,
            flags,
            0,
            ptr::null(),
            0,
            ptr::null(),
            1,
            &image,
        );
        (vk.copy)(commands, image.image, copy_layout, buffer, 1, &region);
        image = image
            .old_layout(copy_layout)
            .new_layout(original)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .dst_access_mask(memory);
        (vk.barrier)(
            commands,
            transfer,
            all | vk::PipelineStageFlags::HOST,
            flags,
            0,
            ptr::null(),
            1,
            &host,
            1,
            &image,
        );
        if allocation.event != 0 {
            (vk.events.as_ref().unwrap().set)(commands, vk::Event::from_raw(allocation.event), all);
        }
    }
}

pub(super) unsafe fn event_set(native: Handle, event: u64) -> i32 {
    let vk = unsafe { &*native.cast::<Dispatch>() };
    i32::from(
        event != 0
            && unsafe {
                (vk.events.as_ref().unwrap().status)(vk.device, vk::Event::from_raw(event))
            } == vk::Result::EVENT_SET,
    )
}

pub(super) unsafe fn map(native: Handle, memory: u64, bytes: u64, data: *mut Handle) -> i32 {
    let vk = unsafe { &*native.cast::<Dispatch>() };
    unsafe {
        (vk.map)(
            vk.device,
            vk::DeviceMemory::from_raw(memory),
            0,
            bytes,
            vk::MemoryMapFlags::empty(),
            data,
        )
    }
    .as_raw()
}

pub(super) unsafe fn unmap(native: Handle, memory: u64) {
    let vk = unsafe { &*native.cast::<Dispatch>() };
    unsafe { (vk.unmap)(vk.device, vk::DeviceMemory::from_raw(memory)) };
}

pub(super) unsafe fn release(native: Handle, allocation: *const Allocation) {
    let (vk, a) = unsafe { (&*native.cast::<Dispatch>(), &*allocation) };
    unsafe {
        if a.event != 0 {
            (vk.events.as_ref().unwrap().destroy)(
                vk.device,
                vk::Event::from_raw(a.event),
                ptr::null(),
            );
        }
        if a.buffer != 0 {
            (vk.destroy_buffer)(vk.device, vk::Buffer::from_raw(a.buffer), ptr::null());
        }
        if a.memory != 0 {
            (vk.free)(vk.device, vk::DeviceMemory::from_raw(a.memory), ptr::null());
        }
    }
}

#[cfg(not(test))]
pub(super) use crate::input_dump::abi::{
    rsf_input_dump_failed as failed, rsf_input_dump_pixels as pixels,
    rsf_input_dump_reserve as reserve, rsf_input_dump_wants as wants,
    rsf_input_dump_wants_frame as wants_frame,
};
