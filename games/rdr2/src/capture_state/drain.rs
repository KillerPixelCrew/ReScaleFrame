// SPDX-License-Identifier: MIT
//! Pending readback admission using observations from each shot's retained fence.
use super::gpu_leases::{Gpu, take_idle};
use super::recordings::Readback;
use super::{Capture, rsf_rdr2_capture_active};
use std::ffi::c_void;
use std::ptr;

const DRAIN_VERSION: u32 = 1;

#[repr(C)]
struct DrainOps {
    struct_size: u32,
    version: u32,
    sample: unsafe extern "C" fn(*mut c_void, usize, &mut u64) -> *const Readback,
    take: unsafe extern "C" fn(*mut c_void, usize) -> bool,
}

const _: () = {
    assert!(size_of::<DrainOps>() == 8 + 2 * size_of::<usize>());
    assert!(std::mem::offset_of!(DrainOps, sample) == 8);
};

/// The native poller holds the capture mutex throughout this call. It lends the pending shots
/// through sample and transfers a selected lease to its ready batch through take. Each sample
/// must return the live receipt at that index and its retained fence's current completion value.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_capture_collect_ready(
    owner: &Capture,
    context: *mut c_void,
    mut pending_count: usize,
    ops: *const DrainOps,
) -> *mut Gpu {
    if rsf_rdr2_capture_active(owner) || ops.is_null() || context.is_null() {
        return ptr::null_mut();
    }
    let header = ops.cast::<u32>();
    if unsafe { header.read() } < size_of::<DrainOps>() as u32
        || unsafe { header.add(1).read() } != DRAIN_VERSION
    {
        return ptr::null_mut();
    }
    let ops = unsafe { &*ops };
    let mut index = 0;
    while index < pending_count {
        let mut completed = 0;
        let readback = unsafe { (ops.sample)(context, index, &mut completed) };
        if !unsafe { &*readback }.is_complete(completed) {
            index += 1;
            continue;
        }
        // A refused transfer keeps the pending lease and stops the scan until the next poll.
        if !unsafe { (ops.take)(context, index) } {
            break;
        }
        pending_count -= 1;
    }
    if pending_count == 0 {
        unsafe { take_idle(owner) }
    } else {
        ptr::null_mut()
    }
}
