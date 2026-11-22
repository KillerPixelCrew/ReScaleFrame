// SPDX-License-Identifier: MIT
//! Recording transaction for a borrowed Godot graph scope.

use core::ffi::c_void;
use core::{mem::size_of, ptr};

use crate::packet_abi::{
    RSF_GODOT_VK_HANDOFF_ABI_VERSION, rsf_godot_graph_calls, rsf_godot_graph_split_fn,
    rsf_godot_vk_handoff,
};

const INITIALIZATION_FAILED: i32 = -3;
const OUT_OF_HOST_MEMORY: i32 = -1;

/// # Safety
/// The engine must already have verified the active command and timeline support.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_graph_pack(
    out: *mut rsf_godot_vk_handoff,
    family: u32,
    driver: *mut c_void,
    split: rsf_godot_graph_split_fn,
) {
    unsafe {
        out.write(rsf_godot_vk_handoff {
            struct_size: size_of::<rsf_godot_vk_handoff>() as u32,
            abi_version: RSF_GODOT_VK_HANDOFF_ABI_VERSION,
            timeline_enabled: 1,
            queue_family: family,
            user: driver,
            split_record: split,
        })
    };
}

/// # Safety
/// `calls`, its callbacks and any non-null `scope` must remain valid for this call.
/// A non-null `out` must be writable.
/// The caller must resolve the thread-local graph lease and verify the driver's identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_graph_split(
    calls: *const rsf_godot_graph_calls,
    scope: *mut c_void,
    current: *mut c_void,
    timeline: u64,
    previous_done: u64,
    ready: u64,
    done: u64,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return INITIALIZATION_FAILED;
    }
    unsafe { out.write(ptr::null_mut()) };
    if scope.is_null()
        || current.is_null()
        || timeline == 0
        || ready <= previous_done
        || done <= ready
    {
        return INITIALIZATION_FAILED;
    }
    let calls = unsafe { &*calls };
    if unsafe { calls.current.unwrap_unchecked()(scope) } != current {
        return INITIALIZATION_FAILED;
    }

    let mut next = unsafe { calls.pooled.unwrap_unchecked()(scope) };
    if next == 0 {
        // Godot retires commands with their pool and can free a semaphore on its own.
        let semaphore = unsafe { calls.create_semaphore.unwrap_unchecked()(scope) };
        if semaphore == 0 {
            return OUT_OF_HOST_MEMORY;
        }
        next = unsafe { calls.create_command.unwrap_unchecked()(scope) };
        if next == 0 {
            unsafe { calls.free_semaphore.unwrap_unchecked()(scope, semaphore) };
            return OUT_OF_HOST_MEMORY;
        }
        // Transfer both resources to the frame-fenced pool before beginning the buffer.
        unsafe { calls.append_pair.unwrap_unchecked()(scope, next, semaphore) };
    }
    if unsafe { calls.begin.unwrap_unchecked()(scope, next) } == 0 {
        return INITIALIZATION_FAILED;
    }

    // After the source is consumed, its continuation belongs to the graph even if the
    // provider later refuses the frame and needs to record its fallback there.
    unsafe {
        calls.labels.unwrap_unchecked()(scope, 0);
        calls.consume.unwrap_unchecked()(scope, next, timeline, previous_done, ready, done);
        calls.labels.unwrap_unchecked()(scope, 1);
        out.write(calls.continuation.unwrap_unchecked()(scope));
    }
    0
}
