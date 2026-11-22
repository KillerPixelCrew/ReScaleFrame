// SPDX-License-Identifier: MIT
//! What the Vulkan device fixtures share: the loader, the device choice and memory allocation.
use super::TestResult;
use crate::sys::rsf_vk_get_proc;
use ash::vk;
use std::{ffi::CStr, ffi::c_char, ptr};
use windows_sys::Win32::Foundation::{FreeLibrary, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

pub(super) const NO_LOADER: &str = "Vulkan loader unavailable";
pub(super) const NO_DEVICE: &str = "enumerate GPUs";

pub(super) struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

/// The system Vulkan loader, or Streamline's interposer when `streamline` is given.
pub(super) fn entry(streamline: rsf_vk_get_proc) -> TestResult<(Library, ash::Entry)> {
    let path = crate::settings_store::wide("vulkan-1.dll");
    let module =
        unsafe { LoadLibraryExW(path.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if module.is_null() {
        return Err(NO_LOADER.into());
    }
    let library = Library(module);
    let proc = unsafe { GetProcAddress(module, c"vkGetInstanceProcAddr".as_ptr().cast()) }
        .ok_or("missing vkGetInstanceProcAddr")?;
    let get_instance_proc_addr = unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, vk::PFN_vkGetInstanceProcAddr>(
            proc,
        )
    };
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
        .position(|(i, ty)| req.memory_type_bits & (1 << i) != 0 && ty.property_flags.contains(flags))
        .ok_or("Vulkan memory type")? as u32;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(req.size)
        .memory_type_index(index);
    Ok(unsafe { device.allocate_memory(&info, None) }?)
}
