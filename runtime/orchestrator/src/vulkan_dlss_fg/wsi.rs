// SPDX-License-Identifier: MIT
use super::Setup;
use crate::dx12_vulkan_platform::lookup;
use crate::vulkan_fg_copy::{Barrier, rsf_core_vkfg_record_barrier, rsf_core_vkfg_record_copy};
use crate::vulkan_fg_resources_abi::rsf_vkfg_image_copy as ImageCopy;
use ash::vk::{self, Handle as _};
use std::{ffi::c_void, mem};

pub(super) struct Wsi {
    device: vk::Device,
    queue: vk::Queue,
    allocator: *const vk::AllocationCallbacks<'static>,
    create: vk::PFN_vkCreateSwapchainKHR,
    destroy: vk::PFN_vkDestroySwapchainKHR,
    images: vk::PFN_vkGetSwapchainImagesKHR,
    acquire: vk::PFN_vkAcquireNextImageKHR,
    present: vk::PFN_vkQueuePresentKHR,
    idle: vk::PFN_vkDeviceWaitIdle,
    wait_inputs: vk::PFN_vkWaitSemaphores,
    barrier: vk::PFN_vkCmdPipelineBarrier,
    copy: vk::PFN_vkCmdCopyImage,
}

impl Wsi {
    pub(super) fn surface_usage(setup: &Setup, surface: vk::SurfaceKHR) -> Option<u32> {
        let graphics = &setup.graphics;
        let address = lookup(
            graphics.instance_proc,
            graphics.instance,
            c"vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
        )?;
        let query = unsafe {
            mem::transmute::<
                unsafe extern "C" fn(),
                vk::PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR,
            >(address)
        };
        let mut capabilities = vk::SurfaceCapabilitiesKHR::default();
        let result = unsafe {
            query(
                vk::PhysicalDevice::from_raw(graphics.physical_device as u64),
                surface,
                &mut capabilities,
            )
        };
        (result == vk::Result::SUCCESS).then_some(capabilities.supported_usage_flags.as_raw())
    }

    pub(super) fn load(setup: &Setup) -> Option<Self> {
        let graphics = &setup.graphics;
        macro_rules! entry {
            ($name:literal, $ty:ident) => {
                unsafe {
                    mem::transmute::<unsafe extern "C" fn(), vk::$ty>(lookup(
                        graphics.device_proc,
                        graphics.device,
                        $name,
                    )?)
                }
            };
        }
        Some(Self {
            device: vk::Device::from_raw(graphics.device as u64),
            queue: vk::Queue::from_raw(setup.queues[0] as u64),
            allocator: setup.allocator.cast(),
            create: entry!(c"vkCreateSwapchainKHR", PFN_vkCreateSwapchainKHR),
            destroy: entry!(c"vkDestroySwapchainKHR", PFN_vkDestroySwapchainKHR),
            images: entry!(c"vkGetSwapchainImagesKHR", PFN_vkGetSwapchainImagesKHR),
            acquire: entry!(c"vkAcquireNextImageKHR", PFN_vkAcquireNextImageKHR),
            present: entry!(c"vkQueuePresentKHR", PFN_vkQueuePresentKHR),
            idle: entry!(c"vkDeviceWaitIdle", PFN_vkDeviceWaitIdle),
            wait_inputs: entry!(c"vkWaitSemaphores", PFN_vkWaitSemaphores),
            barrier: entry!(c"vkCmdPipelineBarrier", PFN_vkCmdPipelineBarrier),
            copy: entry!(c"vkCmdCopyImage", PFN_vkCmdCopyImage),
        })
    }

    pub(super) fn create(&self, info: &vk::SwapchainCreateInfoKHR<'_>, usage: u32) -> Option<u64> {
        let mut info = *info;
        info.image_usage |= vk::ImageUsageFlags::from_raw(usage);
        // Streamline's create hook takes the LL2 swapchain mutex itself.
        let mut chain = vk::SwapchainKHR::null();
        let result = unsafe { (self.create)(self.device, &info, self.allocator, &mut chain) };
        (result == vk::Result::SUCCESS && !chain.is_null()).then_some(chain.as_raw())
    }

    pub(super) fn destroy(&self, chain: u64) {
        unsafe {
            (self.destroy)(
                self.device,
                vk::SwapchainKHR::from_raw(chain),
                self.allocator,
            )
        };
    }

    pub(super) fn images(&self, chain: u64, count: *mut u32, output: *mut u64) -> i32 {
        unsafe {
            (self.images)(
                self.device,
                vk::SwapchainKHR::from_raw(chain),
                count,
                output.cast(),
            )
        }
        .as_raw()
    }

    pub(super) fn acquire(
        &self,
        chain: u64,
        timeout: u64,
        semaphore: u64,
        fence: u64,
        index: *mut u32,
    ) -> i32 {
        unsafe {
            (self.acquire)(
                self.device,
                vk::SwapchainKHR::from_raw(chain),
                timeout,
                vk::Semaphore::from_raw(semaphore),
                vk::Fence::from_raw(fence),
                index,
            )
        }
        .as_raw()
    }

    pub(super) fn present(&self, info: *const c_void) -> i32 {
        unsafe { (self.present)(self.queue, info.cast()) }.as_raw()
    }

    pub(super) fn idle(&self) -> i32 {
        unsafe { (self.idle)(self.device) }.as_raw()
    }

    pub(super) fn wait_inputs(&self, semaphore: u64, value: u64) -> i32 {
        let semaphores = [vk::Semaphore::from_raw(semaphore)];
        let values = [value];
        let info = vk::SemaphoreWaitInfo::default()
            .semaphores(&semaphores)
            .values(&values);
        unsafe { (self.wait_inputs)(self.device, &info, 15_000_000_000) }.as_raw()
    }
}

pub(super) unsafe extern "C" fn record_barrier(
    user: *mut c_void,
    commands: *mut c_void,
    plan: *const Barrier,
) {
    let wsi = unsafe { &*user.cast::<Wsi>() };
    unsafe {
        rsf_core_vkfg_record_barrier(
            wsi.barrier,
            vk::CommandBuffer::from_raw(commands as u64),
            &*plan,
        )
    };
}

pub(super) unsafe extern "C" fn record_copy(
    user: *mut c_void,
    commands: *mut c_void,
    plan: *const ImageCopy,
) {
    let wsi = unsafe { &*user.cast::<Wsi>() };
    unsafe {
        rsf_core_vkfg_record_copy(
            wsi.copy,
            vk::CommandBuffer::from_raw(commands as u64),
            &*plan,
        )
    };
}
