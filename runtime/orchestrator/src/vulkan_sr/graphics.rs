// SPDX-License-Identifier: MIT
use super::{FfxDeviceEntries, Frame, Handle, Image, Setup, rsf_vk_function};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_create, rsf_core_capture_vk_destroy,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_initialize, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire, rsf_core_capture_vk_wants,
};
use crate::vulkan_fg_copy::{
    Barrier, COLOR_RANGE, rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy,
};
use crate::vulkan_fg_resources::device::{
    Device, rsf_core_vkfg_bind_images, rsf_core_vkfg_device_destroy, rsf_core_vkfg_device_memory,
};
use crate::vulkan_fg_resources_abi::rsf_vkfg_image_copy as ImageCopy;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use crate::vulkan_sharpening_abi::rsf_sharpenvk_host as Sharpen;
use ash::vk::{self, Handle as _};
use rsf_game_sys::rsf_frame_record;
use std::{ffi::c_char, ptr};

struct Graphics {
    images: *mut Device,
    capture: *mut Capture,
    sharpen: Sharpen,
    memory_types: u32,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    clear: vk::PFN_vkCmdClearColorImage,
    copy: vk::PFN_vkCmdCopyImage,
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe {
            rsf_sharpenvk_host_destroy(&mut self.sharpen);
            rsf_core_capture_vk_destroy(self.capture);
            rsf_core_vkfg_device_destroy(self.images);
        }
    }
}

fn create(
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> Option<Graphics> {
    // Admission resolves these slots by their exact Vulkan command names.
    macro_rules! procedure {
        ($value:expr, $ty:ident) => {
            unsafe { std::mem::transmute::<unsafe extern "C" fn(), vk::$ty>($value?) }
        };
    }
    let properties = procedure!(properties, PFN_vkGetPhysicalDeviceMemoryProperties);
    let [
        create,
        destroy,
        requirements,
        allocate,
        free,
        bind,
        barrier,
        clear,
        copy,
    ] = *entries;
    let mut graphics = Graphics {
        images: ptr::null_mut(),
        capture: ptr::null_mut(),
        sharpen: Sharpen::default(),
        memory_types: 0,
        barrier: procedure!(barrier, PFN_vkCmdPipelineBarrier),
        clear: procedure!(clear, PFN_vkCmdClearColorImage),
        copy: procedure!(copy, PFN_vkCmdCopyImage),
    };
    graphics.images = rsf_core_vkfg_bind_images(
        setup.device,
        &[create, destroy, requirements, allocate, free, bind],
    );
    let images = unsafe { graphics.images.as_mut() }?;
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        properties(
            vk::PhysicalDevice::from_raw(setup.physical_device as usize as u64),
            &mut memory,
        );
    }
    graphics.memory_types = memory.memory_type_count;
    rsf_core_vkfg_device_memory(images, &memory);
    graphics.capture = rsf_core_capture_vk_create(1);
    let ready = unsafe {
        rsf_core_capture_vk_initialize(graphics.capture, setup.device, setup.device_proc, &memory)
    };
    (ready != 0).then_some(graphics)
}

fn command_buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as usize as u64)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_create(
    setup: &Setup,
    entries: &FfxDeviceEntries,
    properties: rsf_vk_function,
) -> *mut Graphics {
    create(setup, entries, properties).map_or(ptr::null_mut(), crate::allocate_handle)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_destroy(graphics: *mut Graphics) {
    if !graphics.is_null() {
        drop(unsafe { Box::from_raw(graphics) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_images(graphics: &Graphics) -> Handle {
    graphics.images.cast()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_srvk_graphics_memory_count(graphics: &Graphics) -> u32 {
    graphics.memory_types
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_barrier(
    graphics: &Graphics,
    commands: Handle,
    plan: &Barrier,
) {
    unsafe { rsf_core_vkfg_record_barrier(graphics.barrier, command_buffer(commands), plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_clear(
    graphics: &Graphics,
    commands: Handle,
    image: u64,
    layout: u32,
) {
    unsafe {
        (graphics.clear)(
            command_buffer(commands),
            vk::Image::from_raw(image),
            vk::ImageLayout::from_raw(layout as i32),
            &vk::ClearColorValue { uint32: [0; 4] },
            1,
            &COLOR_RANGE,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_copy(
    graphics: &Graphics,
    commands: Handle,
    plan: &ImageCopy,
) {
    unsafe { rsf_core_vkfg_record_copy(graphics.copy, command_buffer(commands), plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_sharpen(
    graphics: &mut Graphics,
    setup: &Setup,
    commands: Handle,
    frame: &Frame,
    output: u64,
) -> i32 {
    i32::from(
        unsafe { rsf_sharpenvk_host_run(&mut graphics.sharpen, setup, commands, frame, output) }
            != 0,
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_clear_sharpen(graphics: *mut Graphics) {
    if let Some(graphics) = unsafe { graphics.as_mut() } {
        unsafe { rsf_sharpenvk_host_clear(&mut graphics.sharpen) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_provider(
    graphics: &Graphics,
    version: u64,
    backend: u32,
) {
    unsafe { rsf_core_capture_vk_provider(graphics.capture, version, backend, 0) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_record(
    graphics: &Graphics,
    commands: Handle,
    image: &Image,
    record: &rsf_frame_record,
    role: *const c_char,
    reset: u32,
) {
    unsafe {
        rsf_core_capture_vk_record(
            graphics.capture,
            commands,
            image,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        )
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_capture_wants(graphics: &Graphics, frame: u64) -> i32 {
    unsafe { rsf_core_capture_vk_wants(graphics.capture, frame) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_retire(graphics: &mut Graphics, commands: Handle) {
    unsafe {
        rsf_sharpenvk_host_retire(&mut graphics.sharpen, commands);
        rsf_core_capture_vk_retire(graphics.capture, commands);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_graphics_flush(graphics: *const Graphics) {
    if let Some(graphics) = unsafe { graphics.as_ref() } {
        unsafe { rsf_core_capture_vk_flush(graphics.capture, 0, ptr::null_mut()) };
    }
}
