// SPDX-License-Identifier: MIT
//! Recording transaction for a borrowed Godot graph scope.

use core::ffi::c_void;
use core::ptr;

const INITIALIZATION_FAILED: i32 = -3;
const OUT_OF_HOST_MEMORY: i32 = -1;

#[repr(C)]
pub struct GraphCalls {
    current: unsafe extern "C" fn(scope: *mut c_void) -> *mut c_void,
    pooled: unsafe extern "C" fn(scope: *mut c_void) -> u64,
    allocate: unsafe extern "C" fn(scope: *mut c_void) -> u64,
    begin: unsafe extern "C" fn(scope: *mut c_void, next: u64) -> u32,
    labels: unsafe extern "C" fn(scope: *mut c_void, reopen: u32),
    consume: unsafe extern "C" fn(
        scope: *mut c_void,
        next: u64,
        timeline: u64,
        previous_done: u64,
        ready: u64,
        done: u64,
    ),
    continuation: unsafe extern "C" fn(scope: *mut c_void) -> *mut c_void,
}

/// # Safety
/// `calls` and `scope` must remain valid for this callback. A non-null `out` must be writable.
/// The caller must resolve the thread-local graph lease and verify the driver's identity.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_godot_graph_split(
    calls: *const GraphCalls,
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
    if unsafe { (calls.current)(scope) } != current {
        return INITIALIZATION_FAILED;
    }

    let mut next = unsafe { (calls.pooled)(scope) };
    if next == 0 {
        next = unsafe { (calls.allocate)(scope) };
        if next == 0 {
            return OUT_OF_HOST_MEMORY;
        }
    }
    if unsafe { (calls.begin)(scope, next) } == 0 {
        return INITIALIZATION_FAILED;
    }

    // After the source is consumed, its continuation belongs to the graph even if the
    // provider later refuses the frame and needs to record its fallback there.
    unsafe {
        (calls.labels)(scope, 0);
        (calls.consume)(scope, next, timeline, previous_done, ready, done);
        (calls.labels)(scope, 1);
        out.write((calls.continuation)(scope));
    }
    0
}
