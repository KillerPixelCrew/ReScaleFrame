// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::dx12_vulkan_platform::lookup;
use crate::frame_capture_vk::{
    Owner as Capture, rsf_core_capture_vk_complete_group, rsf_core_capture_vk_discard_group,
    rsf_core_capture_vk_flush, rsf_core_capture_vk_pending, rsf_core_capture_vk_ready,
    rsf_core_capture_vk_record,
};
use crate::input_dump::abi::rsf_core_dump_wants;
use crate::sr_session::has_capacity;
use crate::sys::{
    RSF_SRVK_ABI_VERSION, rsf_input_dump_vk_setup as Setup, rsf_srvk_image as Image,
    rsf_srvk_setup as Graphics,
};
use crate::vulkan_capture_commands::Commands;
use ash::vk::{self, Handle as _};
use rsf_game_sys::backend_result::{FEATURE_FAILED, INIT_FAILED, INVALID_ARGUMENT, NOT_READY};
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{c_char, c_void};
use std::mem;
use std::ptr;
use std::sync::Mutex;

const DEVICE_LOST: i32 = -13; // rsf_input_dump_vk result
const WAIT_NS: u64 = 15_000_000_000;

type Handle = *mut c_void;
type Submit = unsafe extern "C" fn(Handle, u32, *const c_void, u64) -> i32;

#[derive(Clone, Copy)]
enum Pending {
    Snapshot { group: u64 },
    // An empty drain or an uncertain submit holds the fence without pixel evidence.
    FenceOnly,
}

struct Owner {
    ops: Commands,
    capture: Capture,
    lost: bool,
    device: Handle,
    submit: Submit,
    user: Handle,
    pool: vk::CommandPool,
    commands: vk::CommandBuffer,
    fence: vk::Fence,
    last_group: u64,
    pending: Option<Pending>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        // The exported destroy call joins pending work before consuming the owner.
        if self.pool != vk::CommandPool::null() {
            self.ops.destroy_pool(self.pool);
        }
        if self.fence != vk::Fence::null() {
            self.ops.destroy_fence(self.fence);
        }
    }
}

impl Owner {
    fn create(setup: &Setup, submit: Submit) -> Result<Self, i32> {
        let graphics = &setup.graphics;
        let ops = Commands::load(graphics).ok_or(-7)?;
        let properties = lookup(
            graphics.instance_proc,
            graphics.instance,
            c"vkGetPhysicalDeviceMemoryProperties",
        )
        .ok_or(-7)?;
        let properties = unsafe {
            mem::transmute::<unsafe extern "C" fn(), vk::PFN_vkGetPhysicalDeviceMemoryProperties>(
                properties,
            )
        };
        let mut memory = vk::PhysicalDeviceMemoryProperties::default();
        unsafe {
            properties(
                vk::PhysicalDevice::from_raw(graphics.physical_device as u64),
                &mut memory,
            )
        };
        let mut capture = Capture::new_native(false);
        if !capture.initialize(graphics.device, graphics.device_proc, &memory)
            || unsafe { rsf_core_capture_vk_ready(&capture) } == 0
        {
            return Err(-7);
        }
        let mut owner = Self {
            ops,
            capture,
            lost: false,
            device: graphics.device,
            submit,
            user: setup.user,
            pool: vk::CommandPool::null(),
            commands: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            last_group: 0,
            pending: None,
        };
        owner.pool = owner
            .ops
            .create_pool(
                setup.queue_family,
                vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER,
            )
            .map_err(|result| error_code(result, INIT_FAILED))?;
        owner.commands = owner
            .ops
            .allocate(owner.pool)
            .map_err(|result| error_code(result, INIT_FAILED))?;
        owner.fence = owner
            .ops
            .create_fence()
            .map_err(|result| error_code(result, INIT_FAILED))?;
        Ok(owner)
    }

    fn join(&mut self, timeout: u64) -> Result<bool, i32> {
        let Some(pending) = self.pending else {
            return if self.lost {
                Err(DEVICE_LOST)
            } else {
                Ok(true)
            };
        };
        let result = self.ops.wait_fence(self.fence, timeout);
        if result == vk::Result::ERROR_DEVICE_LOST {
            if let Pending::Snapshot { group } = pending {
                unsafe { rsf_core_capture_vk_discard_group(&self.capture, group) };
            }
            self.pending = None;
            self.lost = true;
            return Err(DEVICE_LOST);
        }
        checked(result, NOT_READY)?;
        self.pending = None;
        if self.lost {
            return Err(DEVICE_LOST);
        }
        if let Pending::Snapshot { group } = pending {
            unsafe { rsf_core_capture_vk_complete_group(&self.capture, group) };
        }
        Ok(unsafe { rsf_core_capture_vk_flush(&self.capture, 0, ptr::null_mut()) } != 0)
    }

    fn abandon(&mut self, group: u64, error: i32) -> i32 {
        unsafe { rsf_core_capture_vk_discard_group(&self.capture, group) };
        if error == DEVICE_LOST {
            self.lost = true;
        } else {
            // Unsubmitted commands cannot be using the discarded allocations.
            unsafe { rsf_core_capture_vk_flush(&self.capture, 0, ptr::null_mut()) };
        }
        error
    }

    fn submit(&self, submits: &[vk::SubmitInfo<'_>]) -> i32 {
        let infos = if submits.is_empty() {
            ptr::null()
        } else {
            submits.as_ptr().cast()
        };
        unsafe { (self.submit)(self.user, submits.len() as u32, infos, self.fence.as_raw()) }
    }

    fn reset_fence(&self) -> vk::Result {
        self.ops.reset_fence(self.fence)
    }

    fn snapshot(&mut self, record: *const Record, image: &Image, name: *const c_char) -> i32 {
        if let Err(code) = self.join(0) {
            return code;
        }
        if image.device != self.device || image.copy_source == 0 || image.image == 0 {
            return INVALID_ARGUMENT;
        }
        if unsafe { rsf_core_capture_vk_pending(&self.capture) } != 0 {
            return FEATURE_FAILED;
        }
        let Some(group) = self.last_group.checked_add(1) else {
            return FEATURE_FAILED;
        };
        self.last_group = group;
        let prepared = checked(self.ops.reset_pool(self.pool), FEATURE_FAILED)
            .and_then(|()| checked(self.reset_fence(), FEATURE_FAILED))
            .and_then(|()| checked(self.ops.begin(self.commands), FEATURE_FAILED));
        if let Err(error) = prepared {
            return self.abandon(group, error);
        }
        let recorded = unsafe {
            rsf_core_capture_vk_record(
                &self.capture,
                self.commands.as_raw() as Handle,
                image,
                record,
                name,
                (*record).flags & rsf_game_sys::RSF_FRAME_FLAG_RESET,
                1,
                ptr::null_mut(),
                group,
            )
        };
        if recorded == 0 {
            return self.abandon(group, FEATURE_FAILED);
        }
        if let Err(error) = checked(self.ops.end(self.commands), FEATURE_FAILED) {
            return self.abandon(group, error);
        }
        if unsafe { rsf_core_capture_vk_pending(&self.capture) } == 0 {
            return 0;
        }
        let submit =
            vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&self.commands));
        if let Err(error) = checked(vk::Result::from_raw(self.submit(&[submit])), FEATURE_FAILED) {
            if error == DEVICE_LOST {
                // A lost submit can still put the command buffer in use.
                self.pending = Some(Pending::FenceOnly);
            }
            return self.abandon(group, error);
        }
        self.pending = Some(Pending::Snapshot { group });
        join_result(self.join(WAIT_NS))
    }

    fn drain(&mut self) -> i32 {
        let published = match self.join(WAIT_NS) {
            Ok(published) => published,
            Err(code) => return code,
        };
        if let Err(error) = checked(self.reset_fence(), FEATURE_FAILED) {
            self.lost = error == DEVICE_LOST;
            return error;
        }
        if let Err(error) = checked(vk::Result::from_raw(self.submit(&[])), FEATURE_FAILED) {
            if error == DEVICE_LOST {
                self.lost = true;
                self.pending = Some(Pending::FenceOnly);
            }
            return error;
        }
        self.pending = Some(Pending::FenceOnly);
        join_result(self.join(WAIT_NS).map(|drained| published && drained))
    }
}

fn error_code(result: vk::Result, fallback: i32) -> i32 {
    if result == vk::Result::ERROR_DEVICE_LOST {
        DEVICE_LOST
    } else {
        fallback
    }
}

fn checked(result: vk::Result, fallback: i32) -> Result<(), i32> {
    if result == vk::Result::SUCCESS {
        Ok(())
    } else {
        Err(error_code(result, fallback))
    }
}

fn join_result(result: Result<bool, i32>) -> i32 {
    match result {
        Ok(true) => 0,
        Ok(false) => FEATURE_FAILED,
        Err(code) => code,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_vk_create(
    setup: *const Setup,
    out: *mut *mut Mutex<Owner>,
) -> i32 {
    if out.is_null() {
        return INVALID_ARGUMENT;
    }
    unsafe { out.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) } {
        return INVALID_ARGUMENT;
    }
    let setup = unsafe { &*setup };
    let graphics = &setup.graphics;
    let Some(submit) = setup.submit else {
        return INVALID_ARGUMENT;
    };
    if setup.abi_version != 1
        || (graphics.struct_size as usize) < size_of::<Graphics>()
        || graphics.abi_version != RSF_SRVK_ABI_VERSION
        || graphics.instance.is_null()
        || graphics.device.is_null()
        || graphics.physical_device.is_null()
        || graphics.instance_proc.is_none()
        || graphics.device_proc.is_none()
        || setup.queue.is_null()
    {
        return INVALID_ARGUMENT;
    }
    let owner = match Owner::create(setup, submit) {
        Ok(owner) => owner,
        Err(code) => return code,
    };
    let handle = allocate_handle(Mutex::new(owner));
    if handle.is_null() {
        return INIT_FAILED;
    }
    unsafe { out.write(handle) };
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_vk_snapshot(
    owner: *mut Mutex<Owner>,
    record: *const Record,
    image: *const Image,
    name: *const c_char,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID_ARGUMENT;
    };
    if record.is_null() || image.is_null() || name.is_null() {
        return INVALID_ARGUMENT;
    }
    let Ok(mut owner) = owner.try_lock() else {
        return NOT_READY;
    };
    if unsafe { rsf_core_dump_wants(record) } == 0 {
        return 0;
    }
    owner.snapshot(record, unsafe { &*image }, name)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_vk_drain(owner: *mut Mutex<Owner>) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return 0;
    };
    let Ok(mut owner) = owner.try_lock() else {
        return NOT_READY;
    };
    owner.drain()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_input_dump_vk_destroy(owner: *mut Mutex<Owner>) -> i32 {
    let Some(lock) = (unsafe { owner.as_ref() }) else {
        return 0;
    };
    {
        let Ok(mut owner) = lock.try_lock() else {
            return NOT_READY;
        };
        if let Err(code) = owner.join(0)
            && code != DEVICE_LOST
        {
            return code;
        }
    }
    drop(unsafe { Box::from_raw(owner) });
    0
}
