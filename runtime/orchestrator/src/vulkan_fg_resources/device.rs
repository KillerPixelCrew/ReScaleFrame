// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::dx12_vulkan_platform::{device_local_mask, lookup, typed};
use crate::sys::rsf_fgvk_setup;
use crate::sys::{rsf_srvk_setup, rsf_vk_function};
use crate::vulkan_fg_resources::Platform as rsf_vkfg_platform;
use crate::vulkan_timing::{timing_begin, timing_end};
use ash::vk::{self, Handle};
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
};

struct Images {
    create: vk::PFN_vkCreateImage,
    destroy: vk::PFN_vkDestroyImage,
    requirements: vk::PFN_vkGetImageMemoryRequirements,
    allocate: vk::PFN_vkAllocateMemory,
    free: vk::PFN_vkFreeMemory,
    bind: vk::PFN_vkBindImageMemory,
}

impl Images {
    fn load(mut lookup: impl FnMut(&'static std::ffi::CStr) -> rsf_vk_function) -> Option<Self> {
        // The host loader and the supplied entry array both identify the exact Vulkan commands.
        macro_rules! get {
            ($name:literal) => {
                unsafe { typed(lookup($name)) }?
            };
        }
        Some(Self {
            create: get!(c"vkCreateImage"),
            destroy: get!(c"vkDestroyImage"),
            requirements: get!(c"vkGetImageMemoryRequirements"),
            allocate: get!(c"vkAllocateMemory"),
            free: get!(c"vkFreeMemory"),
            bind: get!(c"vkBindImageMemory"),
        })
    }
}

struct Fences {
    create: vk::PFN_vkCreateFence,
    destroy: vk::PFN_vkDestroyFence,
    wait: vk::PFN_vkWaitForFences,
    reset: vk::PFN_vkResetFences,
    submit: unsafe extern "C" fn(*mut c_void, u32, *const c_void, u64) -> i32,
    queue_user: *mut c_void,
}

pub(crate) struct Device {
    handle: vk::Device,
    local_types: u32,
    images: Images,
    views: Option<(vk::PFN_vkCreateImageView, vk::PFN_vkDestroyImageView)>,
    fences: Option<Fences>,
    wait_stage: Option<&'static str>,
    wait_frame: u64,
}

impl Device {
    fn new(handle: *mut c_void, images: Images) -> Self {
        Self {
            handle: vk::Device::from_raw(handle as usize as u64),
            local_types: 0,
            images,
            views: None,
            fences: None,
            wait_stage: None,
            wait_frame: 0,
        }
    }

    pub(crate) fn load(
        graphics: &rsf_srvk_setup,
        fg: Option<&rsf_fgvk_setup>,
        with_view: bool,
    ) -> Option<Self> {
        let lookup = |name: &CStr| lookup(graphics.device_proc, graphics.device, name);
        let mut device = Self::new(graphics.device, Images::load(lookup)?);
        macro_rules! get {
            ($name:literal) => {
                unsafe { typed(lookup($name)) }?
            };
        }
        if with_view {
            device.views = Some((get!(c"vkCreateImageView"), get!(c"vkDestroyImageView")));
        }
        if let Some(fg) = fg {
            device.fences = Some(Fences {
                create: get!(c"vkCreateFence"),
                destroy: get!(c"vkDestroyFence"),
                reset: get!(c"vkResetFences"),
                wait: get!(c"vkWaitForFences"),
                submit: fg.graphics_submit?,
                queue_user: fg.queue_user,
            });
        }
        Some(device)
    }
}

pub(crate) unsafe extern "C" fn rsf_core_vkfg_load_device(
    graphics: &rsf_srvk_setup,
    fg: *const rsf_fgvk_setup,
    with_view: bool,
) -> *mut Device {
    Device::load(graphics, unsafe { fg.as_ref() }, with_view)
        .map_or(ptr::null_mut(), allocate_handle)
}

pub(crate) extern "C" fn rsf_core_vkfg_bind_images(
    handle: *mut c_void,
    entries: &[rsf_vk_function; 6],
) -> *mut Device {
    let mut entries = entries.iter().copied();
    Images::load(|_| entries.next().flatten()).map_or(ptr::null_mut(), |images| {
        allocate_handle(Device::new(handle, images))
    })
}

pub(crate) unsafe extern "C" fn rsf_core_vkfg_device_destroy(device: *mut Device) {
    if !device.is_null() {
        unsafe { drop(Box::from_raw(device)) };
    }
}

pub(crate) extern "C" fn rsf_core_vkfg_device_memory(
    device: &mut Device,
    properties: &vk::PhysicalDeviceMemoryProperties,
) {
    device.local_types = device_local_mask(properties);
}

// Only the FFX FG leaf times its fence waits; vulkan_fg's unit tests replace that provider.
#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe extern "C" fn rsf_core_vkfg_device_wait_timing(
    device: &mut Device,
    stage: *const c_char,
    frame: u64,
) {
    device.wait_stage = if stage.is_null() {
        None
    } else {
        // The leaf supplies a string literal valid for its lifetime.
        unsafe { CStr::from_ptr(stage) }.to_str().ok()
    };
    device.wait_frame = frame;
}

// The C adapter borrows the dispatch table for the duration of each call.
// The resource owner retires all images and fences before dropping that table.
unsafe fn device<'a>(user: *mut c_void) -> &'a Device {
    unsafe { &*user.cast::<Device>() }
}

unsafe extern "C" fn local_memory_types(user: *mut c_void) -> u32 {
    unsafe { device(user) }.local_types
}

unsafe extern "C" fn create_image(
    user: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    out: *mut u64,
) -> i32 {
    let device = unsafe { device(user) };
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .format(vk::Format::from_raw(format as i32))
        .samples(vk::SampleCountFlags::TYPE_1)
        .usage(vk::ImageUsageFlags::from_raw(usage));
    let mut image = vk::Image::null();
    let result = unsafe { (device.images.create)(device.handle, &info, ptr::null(), &mut image) };
    unsafe { *out = image.as_raw() };
    result.as_raw()
}

unsafe extern "C" fn requirements(user: *mut c_void, image: u64, size: *mut u64, types: *mut u32) {
    let device = unsafe { device(user) };
    let mut required = vk::MemoryRequirements::default();
    unsafe {
        (device.images.requirements)(device.handle, vk::Image::from_raw(image), &mut required);
        *size = required.size;
        *types = required.memory_type_bits;
    }
}

unsafe extern "C" fn allocate(user: *mut c_void, size: u64, kind: u32, out: *mut u64) -> i32 {
    let device = unsafe { device(user) };
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(size)
        .memory_type_index(kind);
    let mut memory = vk::DeviceMemory::null();
    let result =
        unsafe { (device.images.allocate)(device.handle, &info, ptr::null(), &mut memory) };
    unsafe { *out = memory.as_raw() };
    result.as_raw()
}

unsafe extern "C" fn bind(user: *mut c_void, image: u64, memory: u64) -> i32 {
    let device = unsafe { device(user) };
    unsafe {
        (device.images.bind)(
            device.handle,
            vk::Image::from_raw(image),
            vk::DeviceMemory::from_raw(memory),
            0,
        )
    }
    .as_raw()
}

unsafe extern "C" fn create_view(user: *mut c_void, image: u64, format: u32, out: *mut u64) -> i32 {
    let device = unsafe { device(user) };
    let (create, _) = device.views.unwrap();
    let info = vk::ImageViewCreateInfo::default()
        .image(vk::Image::from_raw(image))
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(vk::Format::from_raw(format as i32))
        .subresource_range(crate::vulkan_fg_copy::COLOR_RANGE);
    let mut view = vk::ImageView::null();
    let result = unsafe { create(device.handle, &info, ptr::null(), &mut view) };
    unsafe { *out = view.as_raw() };
    result.as_raw()
}

unsafe extern "C" fn destroy_view(user: *mut c_void, view: u64) {
    let device = unsafe { device(user) };
    let (_, destroy) = device.views.unwrap();
    unsafe { destroy(device.handle, vk::ImageView::from_raw(view), ptr::null()) };
}

unsafe extern "C" fn destroy_image(user: *mut c_void, image: u64) {
    let device = unsafe { device(user) };
    unsafe { (device.images.destroy)(device.handle, vk::Image::from_raw(image), ptr::null()) };
}

unsafe extern "C" fn free_memory(user: *mut c_void, memory: u64) {
    let device = unsafe { device(user) };
    unsafe {
        (device.images.free)(
            device.handle,
            vk::DeviceMemory::from_raw(memory),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn create_fence(user: *mut c_void, out: *mut u64) -> i32 {
    let device = unsafe { device(user) };
    let mut fence = vk::Fence::null();
    let result = unsafe {
        (device.fences.as_ref().unwrap().create)(
            device.handle,
            &vk::FenceCreateInfo::default(),
            ptr::null(),
            &mut fence,
        )
    };
    unsafe { *out = fence.as_raw() };
    result.as_raw()
}

unsafe extern "C" fn destroy_fence(user: *mut c_void, fence: u64) {
    let device = unsafe { device(user) };
    unsafe {
        (device.fences.as_ref().unwrap().destroy)(
            device.handle,
            vk::Fence::from_raw(fence),
            ptr::null(),
        )
    };
}

unsafe extern "C" fn wait_fence(user: *mut c_void, fence: u64, timeout: u64) -> i32 {
    let device = unsafe { device(user) };
    let frame = device.wait_frame;
    let start = device.wait_stage.map_or(0, |_| timing_begin());
    let result = unsafe {
        (device.fences.as_ref().unwrap().wait)(
            device.handle,
            1,
            &vk::Fence::from_raw(fence),
            vk::TRUE,
            timeout,
        )
    };
    if let Some(stage) = device.wait_stage {
        timing_end(stage, frame, start);
    }
    result.as_raw()
}

unsafe extern "C" fn reset_fence(user: *mut c_void, fence: u64) -> i32 {
    let device = unsafe { device(user) };
    unsafe {
        (device.fences.as_ref().unwrap().reset)(device.handle, 1, &vk::Fence::from_raw(fence))
    }
    .as_raw()
}

unsafe extern "C" fn submit_fence(user: *mut c_void, fence: u64) -> i32 {
    let fences = unsafe { device(user) }.fences.as_ref().unwrap();
    unsafe { (fences.submit)(fences.queue_user, 0, ptr::null(), fence) }
}

pub(crate) static DEVICE_PLATFORM: rsf_vkfg_platform = rsf_vkfg_platform {
    local_memory_types: Some(local_memory_types),
    create_image: Some(create_image),
    requirements: Some(requirements),
    allocate: Some(allocate),
    bind: Some(bind),
    create_view: Some(create_view),
    destroy_view: Some(destroy_view),
    destroy_image: Some(destroy_image),
    free_memory: Some(free_memory),
    create_fence: Some(create_fence),
    destroy_fence: Some(destroy_fence),
    wait_fence: Some(wait_fence),
    reset_fence: Some(reset_fence),
    submit_fence: Some(submit_fence),
};
