// SPDX-License-Identifier: MIT
use super::{Barrier, Handle, INIT_FAILED, ImageCopy, MISSING_ENTRY_POINT, Setup};
use crate::allocate_handle;
use crate::dx12_vulkan_platform::lookup;
use crate::frame_capture_vk::{Owner as CaptureOwner, rsf_core_capture_vk_initialize};
use crate::vulkan_fg_copy::{rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy};
use crate::vulkan_fg_resources::device::{Device, rsf_core_vkfg_device_memory};
use ash::vk::{self, Handle as _};

pub(super) struct Graphics {
    images: Device,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    copy: vk::PFN_vkCmdCopyImage,
}

impl Graphics {
    fn bind(setup: &Setup) -> Option<(Self, vk::PhysicalDeviceMemoryProperties)> {
        let mut images = Device::load(setup, None, true)?;
        let device = |name| lookup(setup.device_proc, setup.device, name);
        let instance = |name| lookup(setup.instance_proc, setup.instance, name);
        // Host proc lookup supplies the concrete Vulkan type for each exact command.
        let (barrier, copy, properties) = unsafe {
            (
                std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdPipelineBarrier>(
                    device(c"vkCmdPipelineBarrier")?,
                ),
                std::mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkCmdCopyImage>(device(
                    c"vkCmdCopyImage",
                )?),
                std::mem::transmute::<
                    unsafe extern "C" fn(),
                    vk::PFN_vkGetPhysicalDeviceMemoryProperties,
                >(instance(c"vkGetPhysicalDeviceMemoryProperties")?),
            )
        };
        let mut memory = vk::PhysicalDeviceMemoryProperties::default();
        unsafe {
            properties(
                vk::PhysicalDevice::from_raw(setup.physical_device as u64),
                &mut memory,
            )
        };
        rsf_core_vkfg_device_memory(&mut images, &memory);
        Some((
            Self {
                images,
                barrier,
                copy,
            },
            memory,
        ))
    }
}

pub(super) unsafe extern "C" fn rsf_core_xessvk_graphics_bind(
    setup: &Setup,
    capture: *mut CaptureOwner,
    output: &mut *mut Graphics,
) -> i32 {
    let Some((graphics, memory)) = Graphics::bind(setup) else {
        return MISSING_ENTRY_POINT;
    };
    if unsafe { rsf_core_capture_vk_initialize(capture, setup.device, setup.device_proc, &memory) }
        == 0
    {
        return INIT_FAILED;
    }
    *output = allocate_handle(graphics);
    if (*output).is_null() { INIT_FAILED } else { 0 }
}

pub(super) unsafe extern "C" fn rsf_core_xessvk_graphics_destroy(graphics: *mut Graphics) {
    if !graphics.is_null() {
        unsafe { drop(Box::from_raw(graphics)) };
    }
}

pub(super) unsafe extern "C" fn rsf_core_xessvk_graphics_images(graphics: *mut Graphics) -> Handle {
    unsafe { graphics.as_mut() }.map_or(std::ptr::null_mut(), |graphics| {
        (&raw mut graphics.images).cast()
    })
}

pub(super) unsafe extern "C" fn rsf_core_xessvk_graphics_barrier(
    graphics: &Graphics,
    commands: Handle,
    plan: &Barrier,
) {
    unsafe {
        rsf_core_vkfg_record_barrier(
            graphics.barrier,
            vk::CommandBuffer::from_raw(commands as u64),
            plan,
        )
    };
}

pub(super) unsafe extern "C" fn rsf_core_xessvk_graphics_copy(
    graphics: &Graphics,
    commands: Handle,
    plan: &ImageCopy,
) {
    unsafe {
        rsf_core_vkfg_record_copy(
            graphics.copy,
            vk::CommandBuffer::from_raw(commands as u64),
            plan,
        )
    };
}
