// SPDX-License-Identifier: MIT
use super::rsf_vk_function;
use ash::vk::{self, Handle as _};
use rsf_test_support::ignore_pipeline_barrier;
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::mem;

#[derive(Clone, Copy)]
pub(super) struct Readback {
    pub live: bool,
    pub memory: bool,
    copied: bool,
    bytes: [u8; 4],
}

pub(super) struct Queue {
    valid: bool,
    readbacks: [Readback; 16],
    pub buffers: usize,
    pub frees: usize,
    pub maps: usize,
    pub invalid_maps: usize,
    pub pools: usize,
    pub pool_destroys: usize,
    pub fence_destroys: usize,
    pub resets: usize,
    pub submits: usize,
    pub markers: usize,
    pub waits: usize,
    pub copies_recorded: usize,
    pub timeout: u64,
    pub pending: bool,
    pub ready: bool,
    pub missing_wait: bool,
    pub fail_fence: bool,
    pub fail_bind: bool,
    pub fail_end: bool,
    pub submit_result: vk::Result,
    pub lost_on_wait: bool,
    signaled: bool,
    fence_reset: bool,
    recording: u64,
    queued: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            valid: true,
            readbacks: [Readback {
                live: false,
                memory: false,
                copied: false,
                bytes: [64, 128, 255, 0],
            }; 16],
            buffers: 0,
            frees: 0,
            maps: 0,
            invalid_maps: 0,
            pools: 0,
            pool_destroys: 0,
            fence_destroys: 0,
            resets: 0,
            submits: 0,
            markers: 0,
            waits: 0,
            copies_recorded: 0,
            timeout: 0,
            pending: false,
            ready: true,
            missing_wait: false,
            fail_fence: false,
            fail_bind: false,
            fail_end: false,
            submit_result: vk::Result::SUCCESS,
            lost_on_wait: false,
            signaled: false,
            fence_reset: false,
            recording: 0,
            queued: 0,
        }
    }
}

impl Queue {
    pub fn last(&self) -> Readback {
        self.readbacks[self.buffers - 1]
    }
    fn check(&mut self, condition: bool, message: &str) {
        if !condition {
            eprintln!("{message}");
            self.valid = false;
        }
    }
    pub fn valid(&self) -> bool {
        self.valid
    }
}

const POOL: u64 = 11;
const FENCE: u64 = 12;
type Callbacks<'a> = *const vk::AllocationCallbacks<'a>;

unsafe fn queue<'a>(handle: impl vk::Handle) -> &'a mut Queue {
    // Single-threaded fixture. Each call ends its borrow before the next call.
    unsafe { &mut *(*(handle.as_raw() as *const UnsafeCell<Queue>)).get() }
}

unsafe extern "system" fn create_pool(
    device: vk::Device,
    info: *const vk::CommandPoolCreateInfo<'_>,
    _: Callbacks<'_>,
    out: *mut vk::CommandPool,
) -> vk::Result {
    let (q, info) = unsafe { (queue(device), &*info) };
    q.check(
        info.queue_family_index == 7
            && info.flags == vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
        "isolate the command pool on the configured family",
    );
    q.pools += 1;
    unsafe { out.write(vk::CommandPool::from_raw(POOL)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_pool(
    device: vk::Device,
    pool: vk::CommandPool,
    _: Callbacks<'_>,
) {
    let q = unsafe { queue(device) };
    q.check(
        pool.as_raw() == POOL && !q.pending,
        "destroy only the retired pool",
    );
    q.pool_destroys += 1;
}

unsafe extern "system" fn allocate_commands(
    device: vk::Device,
    info: *const vk::CommandBufferAllocateInfo<'_>,
    out: *mut vk::CommandBuffer,
) -> vk::Result {
    let (q, info) = unsafe { (queue(device), &*info) };
    q.check(
        info.command_pool.as_raw() == POOL
            && info.level == vk::CommandBufferLevel::PRIMARY
            && info.command_buffer_count == 1,
        "allocate one primary command buffer",
    );
    unsafe { out.write(vk::CommandBuffer::from_raw(device.as_raw())) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn reset_pool(
    device: vk::Device,
    pool: vk::CommandPool,
    _: vk::CommandPoolResetFlags,
) -> vk::Result {
    let q = unsafe { queue(device) };
    q.check(
        pool.as_raw() == POOL && !q.pending,
        "reset only a retired pool",
    );
    q.resets += 1;
    q.recording = 0;
    vk::Result::SUCCESS
}

unsafe extern "system" fn begin(
    commands: vk::CommandBuffer,
    info: *const vk::CommandBufferBeginInfo<'_>,
) -> vk::Result {
    let q = unsafe { queue(commands) };
    q.check(
        unsafe { (*info).flags } == vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT,
        "record one-time commands",
    );
    vk::Result::SUCCESS
}

unsafe extern "system" fn end(commands: vk::CommandBuffer) -> vk::Result {
    if unsafe { queue(commands).fail_end } {
        vk::Result::ERROR_OUT_OF_HOST_MEMORY
    } else {
        vk::Result::SUCCESS
    }
}

unsafe extern "system" fn create_fence(
    device: vk::Device,
    info: *const vk::FenceCreateInfo<'_>,
    _: Callbacks<'_>,
    out: *mut vk::Fence,
) -> vk::Result {
    let q = unsafe { queue(device) };
    q.check(
        unsafe { (*info).flags.is_empty() },
        "create an initially unsignaled fence",
    );
    if q.fail_fence {
        return vk::Result::ERROR_OUT_OF_HOST_MEMORY;
    }
    unsafe { out.write(vk::Fence::from_raw(FENCE)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_fence(device: vk::Device, fence: vk::Fence, _: Callbacks<'_>) {
    let q = unsafe { queue(device) };
    q.check(
        fence.as_raw() == FENCE && !q.pending,
        "destroy only a retired fence",
    );
    q.fence_destroys += 1;
}

unsafe extern "system" fn reset_fences(
    device: vk::Device,
    count: u32,
    fences: *const vk::Fence,
) -> vk::Result {
    let q = unsafe { queue(device) };
    q.check(
        count == 1 && unsafe { (*fences).as_raw() } == FENCE && !q.pending,
        "reset just the retired fence",
    );
    q.signaled = false;
    q.fence_reset = true;
    vk::Result::SUCCESS
}

unsafe extern "system" fn wait_fences(
    device: vk::Device,
    count: u32,
    fences: *const vk::Fence,
    all: vk::Bool32,
    timeout: u64,
) -> vk::Result {
    let q = unsafe { queue(device) };
    q.check(
        count == 1 && unsafe { (*fences).as_raw() } == FENCE && all == vk::TRUE,
        "wait on the exact submitted fence",
    );
    q.waits += 1;
    q.timeout = timeout;
    if q.lost_on_wait {
        q.queued = 0;
        q.pending = false;
        return vk::Result::ERROR_DEVICE_LOST;
    }
    if q.pending && q.ready {
        if q.queued != 0 {
            q.readbacks[(q.queued - 1) as usize].copied = true;
        }
        q.queued = 0;
        q.pending = false;
        q.signaled = true;
    }
    if q.signaled {
        vk::Result::SUCCESS
    } else {
        vk::Result::TIMEOUT
    }
}

pub(super) unsafe extern "C" fn submit(
    user: *mut c_void,
    count: u32,
    values: *const c_void,
    fence: u64,
) -> i32 {
    let q = unsafe { queue(vk::Queue::from_raw(user as u64)) };
    q.check(
        fence == FENCE && q.fence_reset && !q.pending && !q.signaled,
        "submit with a fresh fence on the fixture queue",
    );
    q.fence_reset = false;
    if count == 1 && !values.is_null() {
        let info = unsafe { &*values.cast::<vk::SubmitInfo<'_>>() };
        q.check(
            info.s_type == vk::StructureType::SUBMIT_INFO
                && info.command_buffer_count == 1
                && unsafe { (*info.p_command_buffers).as_raw() } == user as u64
                && info.wait_semaphore_count == 0
                && info.signal_semaphore_count == 0
                && q.recording != 0,
            "submit only the capture command buffer without Present waits",
        );
        q.submits += 1;
    } else {
        q.check(
            count == 0 && values.is_null(),
            "use a fence-only submit to drain",
        );
        q.markers += 1;
    }
    if matches!(
        q.submit_result,
        vk::Result::SUCCESS | vk::Result::ERROR_DEVICE_LOST
    ) {
        q.queued = if count == 0 { 0 } else { q.recording };
        q.pending = true;
    }
    q.submit_result.as_raw()
}

unsafe extern "system" fn properties(
    _: vk::PhysicalDevice,
    out: *mut vk::PhysicalDeviceMemoryProperties,
) {
    let mut info = vk::PhysicalDeviceMemoryProperties {
        memory_type_count: 1,
        ..Default::default()
    };
    info.memory_types[0].property_flags =
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    unsafe { out.write(info) };
}

unsafe extern "system" fn create_buffer(
    device: vk::Device,
    info: *const vk::BufferCreateInfo<'_>,
    _: Callbacks<'_>,
    out: *mut vk::Buffer,
) -> vk::Result {
    let (q, info) = unsafe { (queue(device), &*info) };
    q.check(
        info.size == 4 && info.usage == vk::BufferUsageFlags::TRANSFER_DST,
        "read back one RGBA pixel",
    );
    q.readbacks[q.buffers].live = true;
    q.buffers += 1;
    unsafe { out.write(vk::Buffer::from_raw(q.buffers as u64)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn destroy_buffer(device: vk::Device, buffer: vk::Buffer, _: Callbacks<'_>) {
    let q = unsafe { queue(device) };
    let index = (buffer.as_raw() - 1) as usize;
    q.check(
        q.readbacks[index].live && q.queued != buffer.as_raw(),
        "free a buffer once, after retirement",
    );
    q.readbacks[index].live = false;
}

unsafe extern "system" fn requirements(
    _: vk::Device,
    _: vk::Buffer,
    out: *mut vk::MemoryRequirements,
) {
    unsafe {
        out.write(vk::MemoryRequirements {
            size: 4,
            alignment: 1,
            memory_type_bits: 1,
        })
    };
}

unsafe extern "system" fn allocate_memory(
    device: vk::Device,
    _: *const vk::MemoryAllocateInfo<'_>,
    _: Callbacks<'_>,
    out: *mut vk::DeviceMemory,
) -> vk::Result {
    let q = unsafe { queue(device) };
    q.readbacks[q.buffers - 1].memory = true;
    unsafe { out.write(vk::DeviceMemory::from_raw(q.buffers as u64)) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn free_memory(
    device: vk::Device,
    memory: vk::DeviceMemory,
    _: Callbacks<'_>,
) {
    let q = unsafe { queue(device) };
    let index = (memory.as_raw() - 1) as usize;
    q.check(
        q.readbacks[index].memory && q.queued != memory.as_raw(),
        "keep a submitted allocation through timeout",
    );
    q.readbacks[index].memory = false;
    q.frees += 1;
}

unsafe extern "system" fn bind(
    device: vk::Device,
    _: vk::Buffer,
    _: vk::DeviceMemory,
    _: u64,
) -> vk::Result {
    if unsafe { queue(device).fail_bind } {
        vk::Result::ERROR_OUT_OF_DEVICE_MEMORY
    } else {
        vk::Result::SUCCESS
    }
}

unsafe extern "system" fn map(
    device: vk::Device,
    memory: vk::DeviceMemory,
    offset: u64,
    size: u64,
    _: vk::MemoryMapFlags,
    out: *mut *mut c_void,
) -> vk::Result {
    let q = unsafe { queue(device) };
    let index = (memory.as_raw() - 1) as usize;
    q.check(
        q.readbacks[index].live && q.readbacks[index].memory && offset == 0 && size == 4,
        "map the exact allocation",
    );
    q.maps += 1;
    if !q.readbacks[index].copied {
        q.invalid_maps += 1;
        q.readbacks[index].bytes.fill(0xef);
    }
    unsafe { out.write(q.readbacks[index].bytes.as_mut_ptr().cast()) };
    vk::Result::SUCCESS
}

unsafe extern "system" fn unmap(_: vk::Device, _: vk::DeviceMemory) {}

unsafe extern "system" fn copy(
    commands: vk::CommandBuffer,
    _: vk::Image,
    _: vk::ImageLayout,
    buffer: vk::Buffer,
    _: u32,
    _: *const vk::BufferImageCopy,
) {
    let q = unsafe { queue(commands) };
    q.recording = buffer.as_raw();
    q.copies_recorded += 1;
}

macro_rules! define_proc {
    ($($name:literal => $ty:ident, $callback:ident;)+) => {
        pub(super) unsafe extern "C" fn device_proc(device: *mut c_void, name: *const c_char) -> rsf_vk_function {
            let name = unsafe { CStr::from_ptr(name) };
            if name == c"vkWaitForFences" && unsafe { queue(vk::Device::from_raw(device as u64)).missing_wait } {
                return None;
            }
            $(if name == $name {
                let callback: vk::$ty = $callback;
                return Some(unsafe { mem::transmute::<vk::$ty, unsafe extern "C" fn()>(callback) });
            })+
            None
        }
    };
}

define_proc! {
    c"vkCreateCommandPool" => PFN_vkCreateCommandPool, create_pool;
    c"vkDestroyCommandPool" => PFN_vkDestroyCommandPool, destroy_pool;
    c"vkAllocateCommandBuffers" => PFN_vkAllocateCommandBuffers, allocate_commands;
    c"vkResetCommandPool" => PFN_vkResetCommandPool, reset_pool;
    c"vkBeginCommandBuffer" => PFN_vkBeginCommandBuffer, begin;
    c"vkEndCommandBuffer" => PFN_vkEndCommandBuffer, end;
    c"vkCreateFence" => PFN_vkCreateFence, create_fence;
    c"vkDestroyFence" => PFN_vkDestroyFence, destroy_fence;
    c"vkResetFences" => PFN_vkResetFences, reset_fences;
    c"vkWaitForFences" => PFN_vkWaitForFences, wait_fences;
    c"vkCreateBuffer" => PFN_vkCreateBuffer, create_buffer;
    c"vkDestroyBuffer" => PFN_vkDestroyBuffer, destroy_buffer;
    c"vkGetBufferMemoryRequirements" => PFN_vkGetBufferMemoryRequirements, requirements;
    c"vkAllocateMemory" => PFN_vkAllocateMemory, allocate_memory;
    c"vkFreeMemory" => PFN_vkFreeMemory, free_memory;
    c"vkBindBufferMemory" => PFN_vkBindBufferMemory, bind;
    c"vkMapMemory" => PFN_vkMapMemory, map;
    c"vkUnmapMemory" => PFN_vkUnmapMemory, unmap;
    c"vkCmdPipelineBarrier" => PFN_vkCmdPipelineBarrier, ignore_pipeline_barrier;
    c"vkCmdCopyImageToBuffer" => PFN_vkCmdCopyImageToBuffer, copy;
}

pub(super) unsafe extern "C" fn instance_proc(
    _: *mut c_void,
    name: *const c_char,
) -> rsf_vk_function {
    if unsafe { CStr::from_ptr(name) } != c"vkGetPhysicalDeviceMemoryProperties" {
        return None;
    }
    let callback: vk::PFN_vkGetPhysicalDeviceMemoryProperties = properties;
    Some(unsafe {
        mem::transmute::<vk::PFN_vkGetPhysicalDeviceMemoryProperties, unsafe extern "C" fn()>(
            callback,
        )
    })
}
