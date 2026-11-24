// SPDX-License-Identifier: MIT
use crate::dx12_vulkan_platform::entry;
use crate::sys::rsf_srvk_setup as Graphics;
use ash::vk::{self, Handle as _};
use std::ptr;

macro_rules! command_dispatch {
    ($($field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        pub(super) struct Commands {
            device: vk::Device,
            $($field: vk::$ty,)+
        }

        impl Commands {
            pub(super) fn load(graphics: &Graphics) -> Option<Self> {
                // Each name is paired with its exact system-ABI signature.
                $(let $field = unsafe { entry::<vk::$ty>(graphics.device_proc, graphics.device, $name) }?;)+
                Some(Self {
                    device: vk::Device::from_raw(graphics.device as u64),
                    $($field,)+
                })
            }
        }
    };
}

command_dispatch! {
    create_pool: PFN_vkCreateCommandPool = c"vkCreateCommandPool",
    destroy_pool: PFN_vkDestroyCommandPool = c"vkDestroyCommandPool",
    allocate: PFN_vkAllocateCommandBuffers = c"vkAllocateCommandBuffers",
    reset_pool: PFN_vkResetCommandPool = c"vkResetCommandPool",
    begin: PFN_vkBeginCommandBuffer = c"vkBeginCommandBuffer",
    end: PFN_vkEndCommandBuffer = c"vkEndCommandBuffer",
    create_fence: PFN_vkCreateFence = c"vkCreateFence",
    destroy_fence: PFN_vkDestroyFence = c"vkDestroyFence",
    reset_fences: PFN_vkResetFences = c"vkResetFences",
    wait_fences: PFN_vkWaitForFences = c"vkWaitForFences",
}

impl Commands {
    pub(super) fn create_pool(
        &self,
        family: u32,
        flags: vk::CommandPoolCreateFlags,
    ) -> Result<vk::CommandPool, vk::Result> {
        let info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(family)
            .flags(flags);
        let mut pool = vk::CommandPool::null();
        let result = unsafe { (self.create_pool)(self.device, &info, ptr::null(), &mut pool) };
        result.result_with_success(pool)
    }

    pub(super) fn destroy_pool(&self, pool: vk::CommandPool) {
        unsafe { (self.destroy_pool)(self.device, pool, ptr::null()) };
    }

    pub(super) fn allocate(&self, pool: vk::CommandPool) -> Result<vk::CommandBuffer, vk::Result> {
        let info = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        let mut commands = vk::CommandBuffer::null();
        let result = unsafe { (self.allocate)(self.device, &info, &mut commands) };
        result.result_with_success(commands)
    }

    pub(super) fn reset_pool(&self, pool: vk::CommandPool) -> vk::Result {
        unsafe { (self.reset_pool)(self.device, pool, vk::CommandPoolResetFlags::empty()) }
    }

    pub(super) fn begin(&self, commands: vk::CommandBuffer) -> vk::Result {
        let info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { (self.begin)(commands, &info) }
    }

    pub(super) fn end(&self, commands: vk::CommandBuffer) -> vk::Result {
        unsafe { (self.end)(commands) }
    }

    pub(super) fn create_fence(&self) -> Result<vk::Fence, vk::Result> {
        let mut fence = vk::Fence::null();
        let result = unsafe {
            (self.create_fence)(
                self.device,
                &vk::FenceCreateInfo::default(),
                ptr::null(),
                &mut fence,
            )
        };
        result.result_with_success(fence)
    }

    pub(super) fn destroy_fence(&self, fence: vk::Fence) {
        unsafe { (self.destroy_fence)(self.device, fence, ptr::null()) };
    }

    pub(super) fn reset_fence(&self, fence: vk::Fence) -> vk::Result {
        unsafe { (self.reset_fences)(self.device, 1, &fence) }
    }

    pub(super) fn wait_fence(&self, fence: vk::Fence, timeout: u64) -> vk::Result {
        unsafe { (self.wait_fences)(self.device, 1, &fence, vk::TRUE, timeout) }
    }
}
