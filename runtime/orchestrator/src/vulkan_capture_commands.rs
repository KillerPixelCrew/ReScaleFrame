// SPDX-License-Identifier: MIT
use crate::dx12_vulkan_platform::lookup;
use crate::vulkan_sr_abi::rsf_srvk_setup as Graphics;
use ash::vk::{self, Handle as _};
use std::{ffi::c_void, mem, ptr};

macro_rules! command_dispatch {
    ($($field:ident: $ty:ident = $name:literal),+ $(,)?) => {
        pub(super) struct Commands {
            device: vk::Device,
            $($field: vk::$ty,)+
        }

        impl Commands {
            pub(super) fn load(graphics: &Graphics) -> Option<Self> {
                $(let $field = lookup(graphics.device_proc, graphics.device, $name)?;)+
                Some(Self {
                    device: vk::Device::from_raw(graphics.device as u64),
                    $($field: unsafe { mem::transmute::<unsafe extern "C" fn(), vk::$ty>($field) },)+
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

// Native Streamline WSI uses the same command and fence operations.
#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_commands_open(graphics: &Graphics) -> *mut Commands {
    Commands::load(graphics).map_or(ptr::null_mut(), crate::allocate_handle)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_vk_capture_commands_close(ops: *mut Commands) {
    if !ops.is_null() {
        drop(unsafe { Box::from_raw(ops) });
    }
}

fn write<T>(value: Result<T, vk::Result>, out: &mut T) -> i32 {
    match value {
        Ok(value) => {
            *out = value;
            0
        }
        Err(result) => result.as_raw(),
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_create_pool(ops: &Commands, family: u32, out: &mut u64) -> i32 {
    write(
        ops.create_pool(family, vk::CommandPoolCreateFlags::TRANSIENT)
            .map(|pool| pool.as_raw()),
        out,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_destroy_pool(ops: &Commands, pool: u64) {
    ops.destroy_pool(vk::CommandPool::from_raw(pool));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_allocate(ops: &Commands, pool: u64, out: &mut *mut c_void) -> i32 {
    write(
        ops.allocate(vk::CommandPool::from_raw(pool))
            .map(|commands| commands.as_raw() as *mut c_void),
        out,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_reset_pool(ops: &Commands, pool: u64) -> i32 {
    ops.reset_pool(vk::CommandPool::from_raw(pool)).as_raw()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_begin(ops: &Commands, commands: *mut c_void) -> i32 {
    ops.begin(vk::CommandBuffer::from_raw(commands as u64))
        .as_raw()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_end(ops: &Commands, commands: *mut c_void) -> i32 {
    ops.end(vk::CommandBuffer::from_raw(commands as u64))
        .as_raw()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_create_fence(ops: &Commands, out: &mut u64) -> i32 {
    write(ops.create_fence().map(|fence| fence.as_raw()), out)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_destroy_fence(ops: &Commands, fence: u64) {
    ops.destroy_fence(vk::Fence::from_raw(fence));
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_reset_fence(ops: &Commands, fence: u64) -> i32 {
    ops.reset_fence(vk::Fence::from_raw(fence)).as_raw()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_vk_capture_wait_fence(ops: &Commands, fence: u64, timeout: u64) -> i32 {
    ops.wait_fence(vk::Fence::from_raw(fence), timeout).as_raw()
}
