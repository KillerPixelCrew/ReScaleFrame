// SPDX-License-Identifier: MIT
use super::leaf::guarded;
use super::{Barrier, Frame, Handle, Image, Record, Setup};
use crate::dx12_vulkan_platform::typed;
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_flush, rsf_core_capture_vk_provider,
    rsf_core_capture_vk_record, rsf_core_capture_vk_retire,
};
use crate::input_dump::abi::{rsf_capture_sr_parameters, rsf_input_dump_begin_record};
use crate::sys::{RSF_SR_FSR4, rsf_vk_function};
use crate::vulkan_fg_copy::ImageCopy as Copy;
use crate::vulkan_fg_copy::{rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy};
use crate::vulkan_sharpening::Host as Sharpen;
use crate::vulkan_sharpening::{
    rsf_sharpenvk_host_clear, rsf_sharpenvk_host_destroy, rsf_sharpenvk_host_retire,
    rsf_sharpenvk_host_run,
};
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::FEATURE_FAILED;
use std::ffi::{c_char, c_void};
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
        // Each procedure was resolved by its exact Vulkan command name.
        Some(Self {
            device: vk::Device::from_raw(setup.device as u64),
            memory_properties: unsafe { typed(memory) }?,
            barrier: unsafe { typed(barrier) }?,
            copy: unsafe { typed(copy) }?,
            wait: unsafe { typed(wait) }?,
            idle: unsafe { typed(idle) }?,
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

/// The capsule behind a graphics handle from [`open`].
///
/// # Safety
/// `graphics` must be a live handle from [`open`] that no other reference is using.
unsafe fn capsule<'a>(graphics: Handle) -> &'a mut Graphics {
    unsafe { &mut *graphics.cast::<Graphics>() }
}

/// Resolves the five Vulkan commands through the host's procedure callbacks, which may throw,
/// and opens the capsule.
pub(super) unsafe fn open(setup: *const Setup, out: *mut Handle) -> i32 {
    let setup = unsafe { &*setup };
    let (Some(instance_proc), Some(device_proc)) = (setup.instance_proc, setup.device_proc) else {
        return FEATURE_FAILED;
    };
    // The host callbacks, called with the C-unwind ABI so the guard can contain an exception.
    type Proc = unsafe extern "C" fn(*mut c_void, *const c_char) -> rsf_vk_function;
    type ProcUnwind = unsafe extern "C-unwind" fn(*mut c_void, *const c_char) -> rsf_vk_function;
    let [instance_proc, device_proc] = [instance_proc, device_proc]
        .map(|proc| unsafe { std::mem::transmute::<Proc, ProcUnwind>(proc) });
    let mut entries: [rsf_vk_function; 5] = [None; 5];
    let resolved = guarded(|| {
        entries = unsafe {
            [
                instance_proc(
                    setup.instance,
                    c"vkGetPhysicalDeviceMemoryProperties".as_ptr(),
                ),
                device_proc(setup.device, c"vkCmdPipelineBarrier".as_ptr()),
                device_proc(setup.device, c"vkCmdCopyImage".as_ptr()),
                device_proc(setup.device, c"vkWaitSemaphores".as_ptr()),
                device_proc(setup.device, c"vkDeviceWaitIdle".as_ptr()),
            ]
        };
        0
    });
    if resolved != 0 {
        return resolved;
    }
    let Some(graphics) = Graphics::open(setup, &entries) else {
        return vk::Result::ERROR_EXTENSION_NOT_PRESENT.as_raw();
    };
    let graphics = crate::allocate_handle(graphics);
    if graphics.is_null() {
        return super::OUT_OF_HOST_MEMORY;
    }
    unsafe { out.write(graphics.cast()) };
    0
}

pub(super) unsafe fn close(graphics: Handle) {
    if !graphics.is_null() {
        drop(unsafe { Box::from_raw(graphics.cast::<Graphics>()) });
    }
}

pub(super) unsafe fn capture_initialize(
    graphics: Handle,
    setup: *const Setup,
    provider: u64,
) -> i32 {
    let (graphics, setup) = unsafe { (capsule(graphics), &*setup) };
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
        0
    } else {
        super::INITIALIZATION_FAILED
    }
}

pub(super) unsafe fn capture_begin(record: *const Record) {
    unsafe { rsf_input_dump_begin_record(record, RSF_SR_FSR4) };
}

pub(super) unsafe fn capture_parameters(frame: *const Frame, reset: u32) {
    let frame = unsafe { &*frame };
    unsafe {
        rsf_capture_sr_parameters(
            frame.record,
            RSF_SR_FSR4,
            frame.motion_scale_x,
            frame.motion_scale_y,
            frame.pre_exposure,
            reset,
        );
    }
}

pub(super) unsafe fn capture_record(
    graphics: Handle,
    commands: Handle,
    image: *const Image,
    record: *const Record,
    role: *const c_char,
    reset: u32,
) {
    unsafe {
        rsf_core_capture_vk_record(
            &capsule(graphics).capture,
            commands,
            &*image,
            &*record,
            role,
            reset,
            1,
            ptr::null_mut(),
            0,
        );
    }
}

pub(super) unsafe fn capture_flush(graphics: Handle) -> i32 {
    unsafe { rsf_core_capture_vk_flush(&capsule(graphics).capture, 0, ptr::null_mut()) }
}

pub(super) unsafe fn transition(graphics: Handle, commands: Handle, plan: *const Barrier) {
    let graphics = unsafe { capsule(graphics) };
    unsafe { rsf_core_vkfg_record_barrier(graphics.barrier, command_buffer(commands), &*plan) };
}

pub(super) unsafe fn copy(
    graphics: Handle,
    commands: Handle,
    source: u64,
    target: u64,
    width: u32,
    height: u32,
) {
    let graphics = unsafe { capsule(graphics) };
    let plan = Copy::transfer(source, target, [width, height]);
    unsafe { rsf_core_vkfg_record_copy(graphics.copy, command_buffer(commands), &plan) };
}

pub(super) unsafe fn sharpen(
    graphics: Handle,
    setup: *const Setup,
    commands: Handle,
    frame: *const Frame,
    output: u64,
) {
    let graphics = unsafe { capsule(graphics) };
    unsafe { rsf_sharpenvk_host_run(&mut graphics.sharpen, &*setup, commands, &*frame, output) };
}

pub(super) unsafe fn retire(graphics: Handle, commands: Handle) {
    let graphics = unsafe { capsule(graphics) };
    unsafe {
        rsf_core_capture_vk_retire(&graphics.capture, commands);
        rsf_sharpenvk_host_retire(&mut graphics.sharpen, commands);
    }
}

pub(super) unsafe fn clear_sharpen(graphics: Handle) {
    unsafe { rsf_sharpenvk_host_clear(&mut capsule(graphics).sharpen) };
}

pub(super) unsafe fn wait(graphics: Handle, semaphore: u64, value: u64, timeout: u64) -> i32 {
    let graphics = unsafe { capsule(graphics) };
    let semaphores = [vk::Semaphore::from_raw(semaphore)];
    let values = [value];
    let wait = vk::SemaphoreWaitInfo::default()
        .semaphores(&semaphores)
        .values(&values);
    unsafe { (graphics.wait)(graphics.device, &wait, timeout).as_raw() }
}

pub(super) unsafe fn idle(graphics: Handle) -> i32 {
    let graphics = unsafe { capsule(graphics) };
    unsafe { (graphics.idle)(graphics.device).as_raw() }
}
