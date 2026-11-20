// SPDX-License-Identifier: MIT
use crate::allocate_handle;
use crate::sr_session::has_capacity;
use crate::vulkan_sr_abi::{
    RSF_SRVK_ABI_VERSION, rsf_input_dump_vk_setup as Setup, rsf_srvk_image as Image,
    rsf_srvk_setup as Graphics,
};
use rsf_game_sys::rsf_frame_record as Record;
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Mutex;

const INVALID: i32 = -1;
const INIT_FAILED: i32 = -6;
const NOT_READY: i32 = -8;
const FEATURE_FAILED: i32 = -9;
const DEVICE_LOST: i32 = -4; // VkResult
const WAIT_NS: u64 = 15_000_000_000;

type Handle = *mut c_void;
type Submit = unsafe extern "C" fn(Handle, u32, *const c_void, u64) -> i32;

#[repr(C)]
struct Platform {
    open: unsafe extern "C" fn(*const Graphics, *mut Handle) -> i32,
    close: unsafe extern "C" fn(Handle),
    create_pool: unsafe extern "C" fn(Handle, u32, *mut u64) -> i32,
    destroy_pool: unsafe extern "C" fn(Handle, u64),
    allocate: unsafe extern "C" fn(Handle, u64, *mut Handle) -> i32,
    reset_pool: unsafe extern "C" fn(Handle, u64) -> i32,
    begin: unsafe extern "C" fn(Handle, Handle) -> i32,
    end: unsafe extern "C" fn(Handle, Handle) -> i32,
    create_fence: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    destroy_fence: unsafe extern "C" fn(Handle, u64),
    reset_fence: unsafe extern "C" fn(Handle, u64) -> i32,
    wait_fence: unsafe extern "C" fn(Handle, u64, u64) -> i32,
    submit: unsafe extern "C" fn(Submit, Handle, Handle, u64) -> i32,
    wants: unsafe extern "C" fn(*const Record) -> i32,
    has_readbacks: unsafe extern "C" fn(Handle) -> i32,
    record: unsafe extern "C" fn(
        Handle,
        Handle,
        *const Image,
        *const Record,
        *const c_char,
        u64,
    ) -> i32,
    complete: unsafe extern "C" fn(Handle, u64),
    discard: unsafe extern "C" fn(Handle, u64),
    flush: unsafe extern "C" fn(Handle) -> i32,
    watermark: unsafe extern "C" fn(*const Record) -> u64,
    publish_watermark: unsafe extern "C" fn(Handle, u64),
}

const _: () = {
    assert!(size_of::<Platform>() == 21 * size_of::<Handle>());
};

#[derive(Clone, Copy)]
enum Pending {
    Snapshot { group: u64, watermark: u64 },
    // A drain or an uncertain submission still holds the fence without pixel evidence.
    FenceOnly,
}

struct Owner {
    ops: &'static Platform,
    native: Handle,
    device: Handle,
    submit: Submit,
    user: Handle,
    pool: u64,
    commands: Handle,
    fence: u64,
    last_group: u64,
    pending: Option<Pending>,
}

impl Drop for Owner {
    fn drop(&mut self) {
        // The exported destroy call joins pending work before consuming the owner.
        unsafe {
            if self.pool != 0 {
                (self.ops.destroy_pool)(self.native, self.pool);
            }
            if self.fence != 0 {
                (self.ops.destroy_fence)(self.native, self.fence);
            }
            if !self.native.is_null() {
                (self.ops.close)(self.native);
            }
        }
    }
}

impl Owner {
    fn create(setup: &Setup, submit: Submit, ops: &'static Platform) -> Result<Self, i32> {
        let mut owner = Self {
            ops,
            native: ptr::null_mut(),
            device: setup.graphics.device,
            submit,
            user: setup.user,
            pool: 0,
            commands: ptr::null_mut(),
            fence: 0,
            last_group: 0,
            pending: None,
        };
        let code = unsafe { (ops.open)(&setup.graphics, &mut owner.native) };
        if code != 0 {
            return Err(code);
        }
        if unsafe { (ops.create_pool)(owner.native, setup.queue_family, &mut owner.pool) } != 0
            || unsafe { (ops.allocate)(owner.native, owner.pool, &mut owner.commands) } != 0
            || unsafe { (ops.create_fence)(owner.native, &mut owner.fence) } != 0
        {
            return Err(INIT_FAILED);
        }
        Ok(owner)
    }

    fn join(&mut self, timeout: u64) -> Result<bool, i32> {
        let Some(pending) = self.pending else {
            return Ok(true);
        };
        let code = unsafe { (self.ops.wait_fence)(self.native, self.fence, timeout) };
        if code != 0 {
            if code == DEVICE_LOST {
                if let Pending::Snapshot { group, .. } = pending {
                    unsafe { (self.ops.discard)(self.native, group) };
                }
                self.pending = Some(Pending::FenceOnly);
            }
            return Err(NOT_READY);
        }
        self.pending = None;
        if let Pending::Snapshot { group, watermark } = pending {
            unsafe {
                (self.ops.complete)(self.native, group);
                // Earlier engine readers still use the shared queue watermark.
                (self.ops.publish_watermark)(self.device, watermark);
            }
        }
        Ok(unsafe { (self.ops.flush)(self.native) != 0 })
    }

    fn abandon(&mut self, group: u64, may_be_pending: bool) -> i32 {
        unsafe { (self.ops.discard)(self.native, group) };
        if may_be_pending {
            self.pending = Some(Pending::FenceOnly);
        } else {
            // Unsubmitted commands cannot be using the discarded allocations.
            unsafe { (self.ops.flush)(self.native) };
        }
        FEATURE_FAILED
    }

    fn snapshot(&mut self, record: *const Record, image: &Image, name: *const c_char) -> i32 {
        if let Err(code) = self.join(0) {
            return code;
        }
        if image.device != self.device || image.copy_source == 0 || image.image == 0 {
            return INVALID;
        }
        if unsafe { (self.ops.has_readbacks)(self.native) } != 0 {
            return FEATURE_FAILED;
        }
        let Some(group) = self.last_group.checked_add(1) else {
            return FEATURE_FAILED;
        };
        self.last_group = group;
        let ops = self.ops;
        if unsafe { (ops.reset_pool)(self.native, self.pool) } != 0
            || unsafe { (ops.reset_fence)(self.native, self.fence) } != 0
            || unsafe { (ops.begin)(self.native, self.commands) } != 0
        {
            return FEATURE_FAILED;
        }
        let recorded =
            unsafe { (ops.record)(self.native, self.commands, image, record, name, group) };
        if recorded < 0 || unsafe { (ops.end)(self.native, self.commands) } != 0 {
            return self.abandon(group, false);
        }
        if recorded == 0 {
            return 0;
        }
        let watermark = unsafe { (ops.watermark)(record) };
        let code = unsafe { (ops.submit)(self.submit, self.user, self.commands, self.fence) };
        if code != 0 {
            // Vulkan may leave a submission in use if it reports device loss.
            return self.abandon(group, code == DEVICE_LOST);
        }
        self.pending = Some(Pending::Snapshot { group, watermark });
        join_result(self.join(WAIT_NS))
    }

    fn drain(&mut self) -> i32 {
        let published = match self.join(WAIT_NS) {
            Ok(published) => published,
            Err(code) => return code,
        };
        let ops = self.ops;
        if unsafe { (ops.reset_fence)(self.native, self.fence) } != 0 {
            return FEATURE_FAILED;
        }
        let code = unsafe { (ops.submit)(self.submit, self.user, ptr::null_mut(), self.fence) };
        if code != 0 {
            if code == DEVICE_LOST {
                self.pending = Some(Pending::FenceOnly);
            }
            return FEATURE_FAILED;
        }
        self.pending = Some(Pending::FenceOnly);
        join_result(self.join(WAIT_NS).map(|drained| published && drained))
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
unsafe extern "C" fn rsf_core_input_dump_vk_create(
    setup: *const Setup,
    out: *mut *mut Mutex<Owner>,
    ops: &'static Platform,
) -> i32 {
    if out.is_null() {
        return INVALID;
    }
    unsafe { out.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) } {
        return INVALID;
    }
    let setup = unsafe { &*setup };
    let graphics = &setup.graphics;
    let Some(submit) = setup.submit else {
        return INVALID;
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
        return INVALID;
    }
    let owner = match Owner::create(setup, submit, ops) {
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
unsafe extern "C" fn rsf_core_input_dump_vk_snapshot(
    owner: *mut Mutex<Owner>,
    record: *const Record,
    image: *const Image,
    name: *const c_char,
) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return INVALID;
    };
    if record.is_null() || image.is_null() || name.is_null() {
        return INVALID;
    }
    let Ok(mut owner) = owner.try_lock() else {
        return NOT_READY;
    };
    if unsafe { (owner.ops.wants)(record) } == 0 {
        return 0;
    }
    owner.snapshot(record, unsafe { &*image }, name)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_input_dump_vk_drain(owner: *mut Mutex<Owner>) -> i32 {
    let Some(owner) = (unsafe { owner.as_ref() }) else {
        return 0;
    };
    let Ok(mut owner) = owner.try_lock() else {
        return NOT_READY;
    };
    owner.drain()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_input_dump_vk_destroy(owner: *mut Mutex<Owner>) -> i32 {
    let Some(lock) = (unsafe { owner.as_ref() }) else {
        return 0;
    };
    {
        let Ok(mut owner) = lock.try_lock() else {
            return NOT_READY;
        };
        if let Err(code) = owner.join(0) {
            return code;
        }
    }
    drop(unsafe { Box::from_raw(owner) });
    0
}
