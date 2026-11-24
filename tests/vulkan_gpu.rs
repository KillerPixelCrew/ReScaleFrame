// SPDX-License-Identifier: MIT
//! What the Vulkan device fixtures share: the loader, the device choice and memory allocation.
use super::TestResult;
use crate::sys::rsf_vk_get_proc;
use crate::vulkan_fg_copy::COLOR_RANGE;
use ash::vk::{self, Handle as _};
pub(super) use rsf_test_support::Library;
use std::{ffi::CStr, ffi::c_char};

pub(super) const NO_LOADER: &str = "Vulkan loader unavailable";
pub(super) const NO_DEVICE: &str = "enumerate GPUs";

/// The system Vulkan loader, or Streamline's interposer when `streamline` is given.
pub(super) fn entry(streamline: rsf_vk_get_proc) -> TestResult<(Library, ash::Entry)> {
    let library = Library::vulkan(true).map_err(|_| NO_LOADER)?;
    let get_instance_proc_addr = library
        .vulkan_instance_proc()
        .ok_or("missing vkGetInstanceProcAddr")?;
    let get_instance_proc_addr = streamline.map_or(get_instance_proc_addr, |proc| unsafe {
        std::mem::transmute::<_, vk::PFN_vkGetInstanceProcAddr>(proc)
    });
    let entry = unsafe {
        ash::Entry::from_static_fn(ash::StaticFn {
            get_instance_proc_addr,
        })
    };
    Ok((library, entry))
}

/// A device with one graphics and compute queue on the discrete GPU, or the first GPU. Returns the
/// physical device, the device and the queue family.
pub(super) fn create_device(
    instance: &ash::Instance,
    label: &str,
    extensions: &[*const c_char],
) -> TestResult<(vk::PhysicalDevice, ash::Device, u32)> {
    let devices = unsafe { instance.enumerate_physical_devices() }.map_err(|_| NO_DEVICE)?;
    let first = *devices.first().ok_or(NO_DEVICE)?;
    let physical = devices
        .into_iter()
        .find(|&candidate| {
            unsafe { instance.get_physical_device_properties(candidate) }.device_type
                == vk::PhysicalDeviceType::DISCRETE_GPU
        })
        .unwrap_or(first);
    let properties = unsafe { instance.get_physical_device_properties(physical) };
    let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) };
    println!("{label} GPU: {}", name.to_string_lossy());
    let flags = vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE;
    let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
    let family = families
        .iter()
        .position(|entry| entry.queue_flags.contains(flags))
        .ok_or("graphics and compute queue family")? as u32;
    let mut v12 = vk::PhysicalDeviceVulkan12Features::default();
    let mut v11 = vk::PhysicalDeviceVulkan11Features::default();
    let mut features = vk::PhysicalDeviceFeatures2::default()
        .push_next(&mut v12)
        .push_next(&mut v11);
    unsafe { instance.get_physical_device_features2(physical, &mut features) };
    let queues = [vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&[1.0])];
    let info = vk::DeviceCreateInfo::default()
        .push_next(&mut features)
        .queue_create_infos(&queues)
        .enabled_extension_names(extensions);
    let device = unsafe { instance.create_device(physical, &info, None) }?;
    Ok((physical, device, family))
}

/// The loader's and the instance's proc-address entries as the SR setup's C callbacks. The x64
/// system and C ABIs use the same Vulkan calling convention.
pub(super) fn procs(
    entry: &ash::Entry,
    instance: &ash::Instance,
) -> (rsf_vk_get_proc, rsf_vk_get_proc) {
    unsafe {
        (
            Some(std::mem::transmute(
                entry.static_fn().get_instance_proc_addr,
            )),
            Some(std::mem::transmute(instance.fp_v1_0().get_device_proc_addr)),
        )
    }
}

/// A 2D single-sample image on device-local memory. The image is recorded in `images` before its
/// memory is allocated, so the owner frees it even when a later step fails.
pub(super) fn create_image(
    device: &ash::Device,
    memory: &vk::PhysicalDeviceMemoryProperties,
    images: &mut Vec<(vk::Image, vk::DeviceMemory)>,
    extent: vk::Extent3D,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
) -> TestResult<vk::Image> {
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(extent)
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .usage(usage);
    let image = unsafe { device.create_image(&info, None) }?;
    images.push((image, vk::DeviceMemory::null()));
    let req = unsafe { device.get_image_memory_requirements(image) };
    let backing = allocate(device, memory, req, vk::MemoryPropertyFlags::DEVICE_LOCAL)?;
    images
        .last_mut()
        .ok_or("missing Vulkan image allocation")?
        .1 = backing;
    unsafe { device.bind_image_memory(image, backing, 0) }?;
    Ok(image)
}

/// A whole-image layout transition with full memory access on both sides.
pub(super) fn transition(
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    image: u64,
    from: vk::ImageLayout,
    to: vk::ImageLayout,
) {
    let access = vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE;
    let barrier = vk::ImageMemoryBarrier::default()
        .old_layout(from)
        .new_layout(to)
        .src_access_mask(if from == vk::ImageLayout::UNDEFINED {
            vk::AccessFlags::empty()
        } else {
            access
        })
        .dst_access_mask(access)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(vk::Image::from_raw(image))
        .subresource_range(COLOR_RANGE);
    let stages = vk::PipelineStageFlags::ALL_COMMANDS;
    unsafe {
        device.cmd_pipeline_barrier(
            cmd,
            stages,
            stages,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        )
    };
}

/// Clears an image that is in `TRANSFER_DST_OPTIMAL`.
pub(super) fn fill(device: &ash::Device, cmd: vk::CommandBuffer, image: u64, value: [f32; 4]) {
    unsafe {
        device.cmd_clear_color_image(
            cmd,
            vk::Image::from_raw(image),
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &vk::ClearColorValue { float32: value },
            &[COLOR_RANGE],
        )
    };
}

pub(super) fn allocate(
    device: &ash::Device,
    memory: &vk::PhysicalDeviceMemoryProperties,
    req: vk::MemoryRequirements,
    flags: vk::MemoryPropertyFlags,
) -> TestResult<vk::DeviceMemory> {
    let index = memory
        .memory_types
        .iter()
        .take(memory.memory_type_count as usize)
        .enumerate()
        .position(|(i, ty)| {
            req.memory_type_bits & (1 << i) != 0 && ty.property_flags.contains(flags)
        })
        .ok_or("Vulkan memory type")? as u32;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(index);
    Ok(unsafe { device.allocate_memory(&info, None) }?)
}
