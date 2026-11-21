// SPDX-License-Identifier: MIT
//! Opt-in observation of native RDR2 D3D12 results. The original HRESULT and native
//! call frames remain in the ASI's typed detour.
use crate::asi::{self, MH_ERROR_ALREADY_INITIALIZED, MH_OK};
use crate::diagnostics::path_units;
use std::ffi::{OsString, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::slice;
use std::sync::{Mutex, PoisonError};

const STACK_LIMIT: usize = 24;
const MH_ERROR_NOT_INITIALIZED: i32 = 2;

#[repr(C)]
struct Hook {
    detour: *const c_void,
    original: *mut *mut c_void,
    enable: unsafe extern "C" fn(usize) -> i32,
}

const _: () = assert!(size_of::<Hook>() == 24);

struct Trace {
    path: PathBuf,
    image_size: usize,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

/// The native caller lends a terminated path and its typed trampoline slot.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_d3d12_trace_install(
    target: usize,
    image_size: u32,
    path: *const u16,
    hook: &Hook,
) -> i32 {
    let Some(platform) = asi::platform() else {
        return MH_ERROR_NOT_INITIALIZED;
    };
    let initialized = unsafe { (platform.hook_initialize)() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return initialized;
    }
    let created = unsafe { (platform.hook_create)(target, hook.detour, hook.original) };
    if created != MH_OK {
        return created;
    }
    // Publish before the detour becomes reachable. Keep the trampoline and trace settings
    // after a refused enable because rollback must not free a potentially reachable detour.
    let path = PathBuf::from(OsString::from_wide(unsafe { path_units(path) }));
    *TRACE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Trace {
        path,
        image_size: image_size as usize,
    });
    unsafe { (hook.enable)(target) }
}

/// The detour lends the exact caller address and at most 24 captured native stack addresses.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_rdr2_d3d12_trace_record(
    result: i32,
    base: usize,
    caller: usize,
    tick: u64,
    thread: u32,
    stack: *const *mut c_void,
    count: usize,
) {
    if result >= 0 || count > STACK_LIMIT || (count != 0 && stack.is_null()) {
        return;
    }
    let stack = if count == 0 {
        &[]
    } else {
        unsafe { slice::from_raw_parts(stack, count) }
    };
    let trace = TRACE.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(trace) = trace.as_ref() else {
        return;
    };
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&trace.path)
    else {
        return;
    };
    let _ = write!(
        file,
        "tick={tick} tid={thread} hr=0x{:08x} caller_rva=0x{:x} stack=",
        result as u32,
        caller.wrapping_sub(base),
    );
    for &address in stack {
        let address = address.addr();
        if let Some(offset) = address
            .checked_sub(base)
            .filter(|&offset| offset < trace.image_size)
        {
            let _ = write!(file, " RDR2+0x{offset:x}");
        } else {
            // Match the native x64 CRT's fixed-width pointer output.
            let _ = write!(file, " {address:016X}");
        }
    }
    let _ = writeln!(file);
}
