// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::d3d11_output::same_device;
use crate::d3d11_output_abi::rsf_d3d11_output_is_immediate;
use std::ffi::c_void;
use std::ptr;

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const TOO_SMALL: i32 = -3;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const FOREIGN_DEVICE: i32 = -6;

#[repr(C)]
struct Platform {
    buffer_size: unsafe extern "C" fn(Handle, &mut u32) -> u32,
    staging: unsafe extern "C" fn(Handle, u32) -> Handle,
    copy: unsafe extern "C" fn(Handle, Handle, Handle),
    map: unsafe extern "C" fn(Handle, Handle, &mut *const u8) -> u32,
    unmap: unsafe extern "C" fn(Handle, Handle),
}

const _: () = assert!(size_of::<Platform>() == 5 * size_of::<Handle>());

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_read_constant_buffer(
    device: Handle,
    context: Handle,
    buffer: Handle,
    destination: Handle,
    bytes: u32,
    platform: &Platform,
) -> i32 {
    if device.is_null()
        || context.is_null()
        || buffer.is_null()
        || destination.is_null()
        || bytes == 0
    {
        return INVALID;
    }
    let mut width = 0;
    if unsafe { (platform.buffer_size)(buffer, &mut width) } == 0 {
        return INVALID;
    }
    if !unsafe { same_device(device, buffer) && same_device(device, context) } {
        return FOREIGN_DEVICE;
    }
    if width < bytes {
        return TOO_SMALL;
    }
    // Avoid recording a deferred copy that cannot be followed by a read map.
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return MAP_FAILED;
    }
    let Some(staging) = (unsafe { ComRef::from_owned((platform.staging)(device, width)) }) else {
        return STAGING_FAILED;
    };
    let mut data = ptr::null();
    unsafe { (platform.copy)(context, staging.as_ptr(), buffer) };
    if unsafe { (platform.map)(context, staging.as_ptr(), &mut data) } == 0 {
        return MAP_FAILED;
    }
    let result = if data.is_null() {
        MAP_FAILED
    } else {
        unsafe { ptr::copy_nonoverlapping(data, destination.cast::<u8>(), bytes as usize) };
        OK
    };
    unsafe { (platform.unmap)(context, staging.as_ptr()) };
    result
}
