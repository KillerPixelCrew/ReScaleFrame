// SPDX-License-Identifier: MIT
use super::{Barrier, Frame, Handle, Image, Record, Setup};
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_flush, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire,
};
use crate::input_dump::abi::{rsf_core_dump_begin_record, rsf_core_dump_capture_sr_parameters};
use crate::sys::rsf_sharpenvk_host as Sharpen;
use crate::sys::rsf_vkfg_image_copy as Copy;
use crate::sys::{RSF_SR_FSR4, rsf_vk_function};
use crate::vulkan_fg_copy::{rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy};
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use ash::vk::{self, Handle as _};
use std::ffi::c_char;
use std::ptr;

struct Graphics {
    device: vk::Device,
    memory_properties: vk::PFN_vkGetPhysicalDeviceMemoryProperties,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    copy: vk::PFN_vkCmdCopyImage,
    wait: vk::PFN_vkWaitSemaphores,
    idle: vk::PFN_vkDeviceWaitIdle,
    sharpen: Sharpen,
    capture: Capture,
}

impl Graphics {
    fn open(setup: &Setup, entries: &[rsf_vk_function; 5]) -> Option<Self> {
        let [memory, barrier, copy, wait, idle] = *entries;
        macro_rules! typed {
            ($proc:ident, $ty:ident) => {
                // Each procedure was resolved by its exact Vulkan command name.
                unsafe { std::mem::transmute::<unsafe extern "C" fn(), vk::$ty>($proc?) }
            };
        }
        Some(Self {
            device: vk::Device::from_raw(setup.device as u64),
            memory_properties: typed!(memory, PFN_vkGetPhysicalDeviceMemoryProperties),
            barrier: typed!(barrier, PFN_vkCmdPipelineBarrier),
            copy: typed!(copy, PFN_vkCmdCopyImage),
            wait: typed!(wait, PFN_vkWaitSemaphores),
            idle: typed!(idle, PFN_vkDeviceWaitIdle),
            sharpen: Sharpen::default(),
            capture: Capture::new_native(false),
        })
    }
}

impl Drop for Graphics {
    fn drop(&mut self) {
        unsafe { rsf_sharpenvk_host_destroy(&mut self.sharpen) };
    }
}

fn command_buffer(commands: Handle) -> vk::CommandBuffer {
    vk::CommandBuffer::from_raw(commands as u64)
}

unsafe extern "C" {
    fn rsf_core_dump_bind_native_services();
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_open(
    setup: &Setup,
    entries: &[rsf_vk_function; 5],
    out: &mut Handle,
) -> i32 {
    let Some(graphics) = Graphics::open(setup, entries) else {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    };
    let graphics = crate::allocate_handle(graphics);
    if graphics.is_null() {
        return super::OUT_OF_HOST_MEMORY;
    }
    *out = graphics.cast();
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_close(graphics: *mut Graphics) {
    if !graphics.is_null() {
        drop(unsafe { Box::from_raw(graphics) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_capture_initialize(
    graphics: &mut Graphics,
    setup: &Setup,
    provider: u64,
) -> i32 {
    let mut memory = vk::PhysicalDeviceMemoryProperties::default();
    unsafe {
        (graphics.memory_properties)(
            vk::PhysicalDevice::from_raw(setup.physical_device as u64),
            &mut memory,
        );
        rsf_core_capture_vk_provider(&graphics.capture, provider, RSF_SR_FSR4, 0);
    }
    if graphics
        .capture
        .initialize(setup.device, setup.device_proc, &memory)
    {
        unsafe { rsf_core_dump_bind_native_services() };
        0
    } else {
        super::INITIALIZATION_FAILED
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_capture_begin(record: *const Record) {
    unsafe { rsf_core_dump_begin_record(record, RSF_SR_FSR4) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_capture_parameters(frame: &Frame, reset: u32) {
    unsafe {
        rsf_core_dump_capture_sr_parameters(
            frame.record,
            RSF_SR_FSR4,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.pre_exposure,
            reset,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_capture_record(
    graphics: &Graphics,
    commands: Handle,
    image: &Image,
    record: &Record,
    role: *const c_char,
    reset: u32,
) {
    unsafe {
        rsf_core_capture_vk_record(
            &graphics.capture,
            commands,
            image,
            record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_capture_flush(graphics: &Graphics) -> i32 {
    unsafe { rsf_core_capture_vk_flush(&graphics.capture, 0, ptr::null_mut()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_transition(
    graphics: &Graphics,
    commands: Handle,
    plan: &Barrier,
) {
    unsafe { rsf_core_vkfg_record_barrier(graphics.barrier, command_buffer(commands), plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_copy(
    graphics: &Graphics,
    commands: Handle,
    source: u64,
    target: u64,
    width: u32,
    height: u32,
) {
    let plan = Copy::transfer(source, target, [width, height]);
    unsafe { rsf_core_vkfg_record_copy(graphics.copy, command_buffer(commands), &plan) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_sharpen(
    graphics: &mut Graphics,
    setup: &Setup,
    commands: Handle,
    frame: &Frame,
    output: u64,
) {
    unsafe { rsf_sharpenvk_host_run(&mut graphics.sharpen, setup, commands, frame, output) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_retire(graphics: &mut Graphics, commands: Handle) {
    unsafe {
        rsf_core_capture_vk_retire(&graphics.capture, commands);
        rsf_sharpenvk_host_retire(&mut graphics.sharpen, commands);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_clear_sharpen(graphics: &mut Graphics) {
    unsafe { rsf_sharpenvk_host_clear(&mut graphics.sharpen) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_wait(
    graphics: &Graphics,
    semaphore: u64,
    value: u64,
    timeout: u64,
) -> i32 {
    let semaphores = [vk::Semaphore::from_raw(semaphore)];
    let values = [value];
    let wait = vk::SemaphoreWaitInfo::default()
        .semaphores(&semaphores)
        .values(&values);
    unsafe { (graphics.wait)(graphics.device, &wait, timeout).as_raw() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fsr4vk_graphics_idle(graphics: &Graphics) -> i32 {
    unsafe { (graphics.idle)(graphics.device).as_raw() }
}
