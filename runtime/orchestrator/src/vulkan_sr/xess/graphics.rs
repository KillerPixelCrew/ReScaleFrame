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

    /// Binds the device entries and initializes `capture` on them. The caller owns the result
    /// through `Box::from_raw`; the image leaves borrow its `images` as their user handle.
    pub(super) unsafe fn open(setup: &Setup, capture: *mut CaptureOwner) -> Result<*mut Self, i32> {
        let (graphics, memory) = Self::bind(setup).ok_or(MISSING_ENTRY_POINT)?;
        if unsafe {
            rsf_core_capture_vk_initialize(capture, setup.device, setup.device_proc, &memory)
        } == 0
        {
            return Err(INIT_FAILED);
        }
        let graphics = allocate_handle(graphics);
        if graphics.is_null() {
            Err(INIT_FAILED)
        } else {
            Ok(graphics)
        }
    }

    pub(super) fn images(&mut self) -> Handle {
        (&raw mut self.images).cast()
    }

    pub(super) unsafe fn barrier(&self, commands: Handle, plan: &Barrier) {
        let commands = vk::CommandBuffer::from_raw(commands as u64);
        unsafe { rsf_core_vkfg_record_barrier(self.barrier, commands, plan) };
    }

    pub(super) unsafe fn copy(&self, commands: Handle, plan: &ImageCopy) {
        let commands = vk::CommandBuffer::from_raw(commands as u64);
        unsafe { rsf_core_vkfg_record_copy(self.copy, commands, plan) };
    }
}
