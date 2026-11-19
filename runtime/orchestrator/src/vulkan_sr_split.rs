// SPDX-License-Identifier: MIT
use crate::sr_session::has_capacity;
use crate::vulkan_sr_abi::*;
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_FRAME_ABI_VERSION};
use std::ffi::c_void;
use std::mem::offset_of;
use std::ptr;

type Handle = *mut c_void;
type Frame = rsf_srvk_frame;
type Setup = rsf_srvk_setup;
type Handoff = rsf_fsr4vk_handoff;
type Info = rsf_srvk_fsr4_provider_info;

const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const NOT_COMPILED: i32 = -3;
const NOT_SUPPORTED: i32 = -7;
const FIRST_RESERVED_QUEUE_FAMILY: u32 = 0xffff_fffd;
const FRAME_PREFIX: usize = offset_of!(Frame, bias_current_color);

#[repr(C)]
struct Provider {
    create: unsafe extern "C" fn(*const Setup, *const Handoff, *mut Handle) -> i32,
    plan: unsafe extern "C" fn(*const c_void, *mut u32, *mut u32) -> i32,
    evaluate: unsafe extern "C" fn(Handle, Handle, *const Frame, *mut Handle) -> i32,
    destroy: unsafe extern "C" fn(Handle) -> i32,
    retire: unsafe extern "C" fn(Handle, Handle),
    submitted: unsafe extern "C" fn(Handle, Handle) -> i32,
    info: unsafe extern "C" fn(*const c_void, *mut Info) -> i32,
}

fn public_result(result: i32) -> i32 {
    match result {
        RSF_FSR4_BRIDGE_INVALID_ARGUMENT => INVALID,
        RSF_FSR4_BRIDGE_ABI_MISMATCH => ABI_MISMATCH,
        _ => result,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_create(
    provider: *const Provider,
    setup: *const Setup,
    handoff: *const Handoff,
    owner: *mut Handle,
) -> i32 {
    if owner.is_null() {
        return INVALID;
    }
    unsafe { owner.write(ptr::null_mut()) };
    if !unsafe { has_capacity(setup) && has_capacity(handoff) } {
        return INVALID;
    }
    let (facts, lease) = unsafe { (&*setup, &*handoff) };
    if facts.abi_version != RSF_SRVK_ABI_VERSION
        || !matches!(
            lease.abi_version,
            RSF_SRVK_SPLIT_ABI_VERSION | RSF_SRVK_SPLIT_TRACKED_ABI_VERSION
        )
    {
        return ABI_MISMATCH;
    }
    if facts.backend != RSF_SR_FSR4 {
        return NOT_SUPPORTED;
    }
    if facts.quality > 4
        || facts.instance.is_null()
        || facts.physical_device.is_null()
        || facts.device.is_null()
        || facts.instance_proc.is_none()
        || facts.device_proc.is_none()
        || facts.output_width == 0
        || facts.output_height == 0
        || facts.runtime_directory_utf8.is_null()
        || unsafe { *facts.runtime_directory_utf8 } == 0
        || lease.timeline_enabled != 1
        || lease.queue_family >= FIRST_RESERVED_QUEUE_FAMILY
        || lease.split_record.is_none()
        || facts.depth_inverted > 1
        || facts.depth_infinite > 1
        || facts.motion_jittered > 1
    {
        return INVALID;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.create)(setup, handoff, owner) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_plan(
    provider: *const Provider,
    owner: *const c_void,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    unsafe {
        if !width.is_null() {
            width.write(0);
        }
        if !height.is_null() {
            height.write(0);
        }
    }
    if owner.is_null() || width.is_null() || height.is_null() {
        return INVALID;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.plan)(owner, width, height) })
}

pub(super) unsafe fn normalize_frame(frame: *const Frame) -> Result<Frame, i32> {
    if frame.is_null() {
        return Err(INVALID);
    }
    let size = unsafe { frame.cast::<u32>().read() } as usize;
    if size != FRAME_PREFIX && size < size_of::<Frame>() {
        return Err(INVALID);
    }
    if unsafe { (*frame).abi_version } != RSF_SRVK_ABI_VERSION {
        return Err(ABI_MISMATCH);
    }
    let mut normalized = std::mem::MaybeUninit::<Frame>::zeroed();
    unsafe {
        ptr::copy_nonoverlapping(
            frame.cast::<u8>(),
            normalized.as_mut_ptr().cast::<u8>(),
            size.min(size_of::<Frame>()),
        );
        let mut frame = normalized.assume_init();
        frame.struct_size = size_of::<Frame>() as u32;
        Ok(frame)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_evaluate(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
    frame: *const Frame,
    continuation: *mut Handle,
) -> i32 {
    if continuation.is_null() {
        return INVALID;
    }
    // Failed admission leaves the host's original command buffer recordable.
    unsafe { continuation.write(commands) };
    if owner.is_null() || commands.is_null() {
        return INVALID;
    }
    let frame = match unsafe { normalize_frame(frame) } {
        Ok(frame) => frame,
        Err(error) => return error,
    };
    if !unsafe { has_capacity(frame.record) } {
        return INVALID;
    }
    if unsafe { (*frame.record).abi_version } != RSF_GAME_FRAME_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let camera = unsafe { ptr::addr_of!((*frame.record).camera) };
    if !unsafe { has_capacity(camera) } {
        return INVALID;
    }
    if unsafe { (*camera).abi_version } != RSF_GAME_CAMERA_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if frame.reactive.image != 0 || frame.composition.image != 0 {
        return NOT_SUPPORTED;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    // The provider owns any committed split. Keep its continuation on error.
    public_result(unsafe { (provider.evaluate)(owner, commands, &frame, continuation) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_destroy(provider: *const Provider, owner: Handle) -> i32 {
    if owner.is_null() {
        return 0;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.destroy)(owner) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_retire_commands(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
) {
    if let Some(provider) = unsafe { provider.as_ref() } {
        unsafe { (provider.retire)(owner, commands) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_submitted(
    provider: *const Provider,
    owner: Handle,
    commands: Handle,
) -> i32 {
    if owner.is_null() || commands.is_null() {
        return INVALID;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.submitted)(owner, commands) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_srvk_fsr4_info(
    provider: *const Provider,
    owner: *const c_void,
    out: *mut Info,
) -> i32 {
    if !unsafe { has_capacity(out) } {
        return INVALID;
    }
    unsafe {
        (*out).adapter_luid = 0;
        (*out).provider_version = 0;
        (*out).vendor_id = 0;
        (*out).int8_compatibility = 0;
    }
    if unsafe { (*out).abi_version } != RSF_SRVK_FSR4_INFO_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if owner.is_null() {
        return INVALID;
    }
    let Some(provider) = (unsafe { provider.as_ref() }) else {
        return NOT_COMPILED;
    };
    public_result(unsafe { (provider.info)(owner, out) })
}

#[repr(C)]
#[derive(Default)]
struct SplitLease {
    tracked: u32,
    producer: Handle,
    continuation: Handle,
}

const _: () = assert!(size_of::<SplitLease>() == 24);

#[unsafe(no_mangle)]
extern "C" fn rsf_core_split_pending(lease: &SplitLease) -> i32 {
    i32::from(lease.tracked != 0 && (!lease.producer.is_null() || !lease.continuation.is_null()))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_split_begin(lease: &mut SplitLease, commands: Handle) {
    if lease.tracked != 0 {
        lease.producer = commands;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_split_resume(lease: &mut SplitLease, commands: Handle) {
    if lease.tracked != 0 {
        lease.continuation = commands;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_split_submitted(lease: &mut SplitLease, commands: Handle) -> i32 {
    if lease.tracked == 0 || commands.is_null() {
        return 0;
    }
    if commands == lease.producer {
        lease.producer = ptr::null_mut();
        return 1;
    }
    if lease.producer.is_null() && commands == lease.continuation {
        lease.continuation = ptr::null_mut();
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_split_requires_ordered_submission_of_both_sections() {
        let mut lease = SplitLease::default();
        let producer = ptr::dangling_mut::<u32>().cast();
        let resumed = ptr::dangling_mut::<u64>().cast();
        rsf_core_split_begin(&mut lease, producer);
        assert_eq!(rsf_core_split_pending(&lease), 0);
        assert_eq!(rsf_core_split_submitted(&mut lease, producer), 0);

        lease.tracked = 1;
        rsf_core_split_begin(&mut lease, producer);
        rsf_core_split_resume(&mut lease, resumed);
        assert_eq!(rsf_core_split_pending(&lease), 1);
        assert_eq!(rsf_core_split_submitted(&mut lease, resumed), 0);
        assert_eq!(
            rsf_core_split_submitted(&mut lease, ptr::dangling_mut::<u8>().cast()),
            0
        );
        assert_eq!(rsf_core_split_submitted(&mut lease, producer), 1);
        assert_eq!(rsf_core_split_pending(&lease), 1);
        assert_eq!(rsf_core_split_submitted(&mut lease, producer), 0);
        assert_eq!(rsf_core_split_submitted(&mut lease, resumed), 1);
        assert_eq!(rsf_core_split_pending(&lease), 0);
        rsf_core_split_begin(&mut lease, producer);
        assert_eq!(rsf_core_split_pending(&lease), 1);
    }
}
