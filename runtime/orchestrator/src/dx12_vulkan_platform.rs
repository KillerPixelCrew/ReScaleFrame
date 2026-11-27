// SPDX-License-Identifier: MIT
// The D3D12-Vulkan importer's Vulkan leaves. The fixture-leaves archive calls the FSR4 SR state
// fixture's doubles instead, so they are unused there.
#![cfg_attr(all(feature = "fixture-leaves", not(test)), allow(dead_code))]
use crate::sys::{rsf_srvk_setup as Setup, rsf_vk_function, rsf_vk_get_proc};
use ash::vk::{self, Handle as _};
use std::ffi::{CStr, c_void};
use std::{mem, ptr};

type Handle = *mut c_void;
const MEMORY_HANDLE: vk::ExternalMemoryHandleTypeFlags =
    vk::ExternalMemoryHandleTypeFlags::D3D12_RESOURCE;
const FENCE_HANDLE: vk::ExternalSemaphoreHandleTypeFlags =
    vk::ExternalSemaphoreHandleTypeFlags::D3D12_FENCE;

pub(super) fn lookup(proc: rsf_vk_get_proc, object: Handle, name: &CStr) -> rsf_vk_function {
    // The setup lends the C resolver and its dispatch object for this call.
    unsafe { proc?(object, name.as_ptr()) }
}

/// Types a resolved Vulkan or SDK entry as its function-pointer type `T`.
///
/// # Safety
///
/// `T` is the exact system-ABI signature of the entry `function` was resolved for.
pub(crate) unsafe fn typed<T: Copy>(function: rsf_vk_function) -> Option<T> {
    const { assert!(size_of::<T>() == size_of::<unsafe extern "C" fn()>()) };
    // The caller pairs the entry name with its signature.
    function.map(|function| unsafe { mem::transmute_copy::<unsafe extern "C" fn(), T>(&function) })
}

/// Resolves the Vulkan command `name` through `proc` for `object`, typed as `T`.
///
/// # Safety
///
/// `T` is the exact system-ABI signature of the command `name`.
pub(crate) unsafe fn entry<T: Copy>(
    proc: rsf_vk_get_proc,
    object: Handle,
    name: &CStr,
) -> Option<T> {
    unsafe { typed(lookup(proc, object, name)) }
}
// Selects the resolver and dispatch object for an `instance` or `device` scoped entry.
macro_rules! scope {
    ($setup:ident, instance) => {
        ($setup.instance_proc, $setup.instance)
    };
    ($setup:ident, device) => {
        ($setup.device_proc, $setup.device)
    };
}

macro_rules! dispatch {
    ($($scope:ident $field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        struct Dispatch {
            physical: vk::PhysicalDevice,
            device: vk::Device,
            $($field: vk::$ty,)+
        }

        impl Dispatch {
            fn load(setup: &Setup) -> Option<Self> {
                // Query all entries before checking for a missing address.
                $(let $field = {
                    let (proc, object) = scope!(setup, $scope);
                    // The Vulkan name and its exact system-ABI signature are paired here.
                    unsafe { entry::<vk::$ty>(proc, object, $name) }
                };)+
                Some(Self {
                    physical: vk::PhysicalDevice::from_raw(setup.physical_device as u64),
                    device: vk::Device::from_raw(setup.device as u64),
                    $($field: $field?,)+
                })
            }
        }
    };
}

dispatch! {
    instance image_properties: PFN_vkGetPhysicalDeviceImageFormatProperties2 = c"vkGetPhysicalDeviceImageFormatProperties2",
    instance memory_properties: PFN_vkGetPhysicalDeviceMemoryProperties = c"vkGetPhysicalDeviceMemoryProperties",
    instance semaphore_properties: PFN_vkGetPhysicalDeviceExternalSemaphoreProperties = c"vkGetPhysicalDeviceExternalSemaphoreProperties",
    device create_image: PFN_vkCreateImage = c"vkCreateImage",
    device destroy_image: PFN_vkDestroyImage = c"vkDestroyImage",
    device requirements: PFN_vkGetImageMemoryRequirements = c"vkGetImageMemoryRequirements",
    device handle_properties: PFN_vkGetMemoryWin32HandlePropertiesKHR = c"vkGetMemoryWin32HandlePropertiesKHR",
    device allocate_memory: PFN_vkAllocateMemory = c"vkAllocateMemory",
    device free_memory: PFN_vkFreeMemory = c"vkFreeMemory",
    device bind_image: PFN_vkBindImageMemory = c"vkBindImageMemory",
    device create_semaphore: PFN_vkCreateSemaphore = c"vkCreateSemaphore",
    device destroy_semaphore: PFN_vkDestroySemaphore = c"vkDestroySemaphore",
    device import_semaphore: PFN_vkImportSemaphoreWin32HandleKHR = c"vkImportSemaphoreWin32HandleKHR",
}

pub(super) unsafe extern "C" fn adapter_luid(setup: *const Setup, out: *mut u64) -> i32 {
    let setup = unsafe { &*setup };
    let Some(get) = (unsafe {
        entry::<vk::PFN_vkGetPhysicalDeviceProperties2>(
            setup.instance_proc,
            setup.instance,
            c"vkGetPhysicalDeviceProperties2",
        )
    }) else {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    };
    let mut identity = vk::PhysicalDeviceIDProperties::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut identity);
    unsafe {
        get(
            vk::PhysicalDevice::from_raw(setup.physical_device as u64),
            &mut properties,
        )
    };
    let luid = u64::from_ne_bytes(identity.device_luid);
    unsafe { out.write(luid) };
    if identity.device_luid_valid != vk::FALSE && luid != 0 {
        0
    } else {
        vk::Result::ERROR_INVALID_EXTERNAL_HANDLE.as_raw()
    }
}

pub(crate) unsafe extern "C" fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    let Some(dispatch) = Dispatch::load(unsafe { &*setup }) else {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    };
    let native: Handle = crate::allocate_handle(dispatch).cast();
    if native.is_null() {
        return vk::Result::ERROR_OUT_OF_HOST_MEMORY.as_raw();
    }
    unsafe { out.write(native) };
    0
}

pub(crate) unsafe extern "C" fn close(opaque: Handle) {
    drop(unsafe { Box::from_raw(opaque.cast::<Dispatch>()) });
}

pub(crate) unsafe extern "C" fn timeline_importable(opaque: Handle) -> u32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut ty = vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
    let mut query = vk::PhysicalDeviceExternalSemaphoreInfo::default().handle_type(FENCE_HANDLE);
    query.p_next = ptr::from_mut(&mut ty).cast();
    let mut props = vk::ExternalSemaphoreProperties::default();
    unsafe { (vk.semaphore_properties)(vk.physical, &query, &mut props) };
    u32::from(
        props
            .external_semaphore_features
            .contains(vk::ExternalSemaphoreFeatureFlags::IMPORTABLE),
    )
}

pub(crate) unsafe extern "C" fn create_timeline(opaque: Handle, out: *mut u64) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut ty = vk::SemaphoreTypeCreateInfo::default().semaphore_type(vk::SemaphoreType::TIMELINE);
    let info = vk::SemaphoreCreateInfo::default().push_next(&mut ty);
    let mut timeline = vk::Semaphore::null();
    let result = unsafe { (vk.create_semaphore)(vk.device, &info, ptr::null(), &mut timeline) };
    if result == vk::Result::SUCCESS {
        unsafe { out.write(timeline.as_raw()) };
    }
    result.as_raw()
}

pub(crate) unsafe extern "C" fn import_timeline(
    opaque: Handle,
    timeline: u64,
    handle: Handle,
) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let info = vk::ImportSemaphoreWin32HandleInfoKHR::default()
        .semaphore(vk::Semaphore::from_raw(timeline))
        .handle_type(FENCE_HANDLE)
        .handle(handle as isize);
    unsafe { (vk.import_semaphore)(vk.device, &info).as_raw() }
}

pub(crate) unsafe extern "C" fn destroy_timeline(opaque: Handle, timeline: u64) {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    unsafe { (vk.destroy_semaphore)(vk.device, vk::Semaphore::from_raw(timeline), ptr::null()) };
}

fn usage(extra: u32) -> vk::ImageUsageFlags {
    vk::ImageUsageFlags::TRANSFER_SRC
        | vk::ImageUsageFlags::TRANSFER_DST
        | vk::ImageUsageFlags::from_raw(extra)
}

pub(crate) unsafe extern "C" fn image_properties(
    opaque: Handle,
    format: u32,
    extra_usage: u32,
    importable: *mut u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut external =
        vk::PhysicalDeviceExternalImageFormatInfo::default().handle_type(MEMORY_HANDLE);
    let query = vk::PhysicalDeviceImageFormatInfo2::default()
        .format(vk::Format::from_raw(format as i32))
        .ty(vk::ImageType::TYPE_2D)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage(extra_usage))
        .push_next(&mut external);
    let mut external_properties = vk::ExternalImageFormatProperties::default();
    let mut properties = vk::ImageFormatProperties2::default().push_next(&mut external_properties);
    let result = unsafe { (vk.image_properties)(vk.physical, &query, &mut properties) };
    if result == vk::Result::SUCCESS {
        let extent = properties.image_format_properties.max_extent;
        let features = external_properties
            .external_memory_properties
            .external_memory_features;
        unsafe {
            importable.write(u32::from(
                features.contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE),
            ));
            width.write(extent.width);
            height.write(extent.height);
        }
    }
    result.as_raw()
}

pub(crate) unsafe extern "C" fn create_image(
    opaque: Handle,
    format: u32,
    width: u32,
    height: u32,
    extra_usage: u32,
    out: *mut u64,
) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(MEMORY_HANDLE);
    let info = vk::ImageCreateInfo::default()
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
        .usage(usage(extra_usage))
        .push_next(&mut external);
    let mut image = vk::Image::null();
    let result = unsafe { (vk.create_image)(vk.device, &info, ptr::null(), &mut image) };
    if result == vk::Result::SUCCESS {
        unsafe { out.write(image.as_raw()) };
    }
    result.as_raw()
}

pub(crate) unsafe extern "C" fn image_requirements(
    opaque: Handle,
    image: u64,
    bytes: *mut u64,
    types: *mut u32,
) {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut requirements = vk::MemoryRequirements::default();
    unsafe {
        (vk.requirements)(vk.device, vk::Image::from_raw(image), &mut requirements);
        bytes.write(requirements.size);
        types.write(requirements.memory_type_bits);
    }
}

pub(crate) unsafe extern "C" fn handle_memory_types(
    opaque: Handle,
    handle: Handle,
    out: *mut u32,
) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut props = vk::MemoryWin32HandlePropertiesKHR::default();
    let result =
        unsafe { (vk.handle_properties)(vk.device, MEMORY_HANDLE, handle as isize, &mut props) };
    if result == vk::Result::SUCCESS {
        unsafe { out.write(props.memory_type_bits) };
    }
    result.as_raw()
}

pub(crate) unsafe extern "C" fn device_local_types(opaque: Handle) -> u32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe { (vk.memory_properties)(vk.physical, &mut memory) };
    device_local_mask(&memory)
}

pub(super) fn device_local_mask(memory: &vk::PhysicalDeviceMemoryProperties) -> u32 {
    memory
        .memory_types
        .iter()
        .take(memory.memory_type_count as usize)
        .enumerate()
        .filter(|(_, ty)| {
            ty.property_flags
                .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .fold(0, |mask, (index, _)| mask | (1 << index))
}

pub(crate) unsafe extern "C" fn allocate_memory(
    opaque: Handle,
    image: u64,
    handle: Handle,
    bytes: u64,
    ty: u32,
    out: *mut u64,
) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    let mut dedicated =
        vk::MemoryDedicatedAllocateInfo::default().image(vk::Image::from_raw(image));
    let mut import = vk::ImportMemoryWin32HandleInfoKHR::default()
        .handle_type(MEMORY_HANDLE)
        .handle(handle as isize);
    // push_next prepends, so the import descriptor points to the dedicated image.
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(bytes)
        .memory_type_index(ty)
        .push_next(&mut dedicated)
        .push_next(&mut import);
    let mut memory = vk::DeviceMemory::null();
    let result = unsafe { (vk.allocate_memory)(vk.device, &info, ptr::null(), &mut memory) };
    if result == vk::Result::SUCCESS {
        unsafe { out.write(memory.as_raw()) };
    }
    result.as_raw()
}

pub(crate) unsafe extern "C" fn bind_image(opaque: Handle, image: u64, memory: u64) -> i32 {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    unsafe {
        (vk.bind_image)(
            vk.device,
            vk::Image::from_raw(image),
            vk::DeviceMemory::from_raw(memory),
            0,
        )
        .as_raw()
    }
}

pub(crate) unsafe extern "C" fn destroy_image(opaque: Handle, image: u64) {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    unsafe { (vk.destroy_image)(vk.device, vk::Image::from_raw(image), ptr::null()) };
}

pub(crate) unsafe extern "C" fn free_memory(opaque: Handle, memory: u64) {
    let vk = unsafe { &*opaque.cast::<Dispatch>() };
    unsafe { (vk.free_memory)(vk.device, vk::DeviceMemory::from_raw(memory), ptr::null()) };
}
