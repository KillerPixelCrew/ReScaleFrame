// SPDX-License-Identifier: MIT
//! Opt-in observation of native RDR2 D3D12 results.
use crate::diagnostics::path_units;
use rsf_game_sys::ffi_util::lock_or_recover;
use rsf_minhook_sys::{
    MH_CreateHook, MH_ERROR_ALREADY_INITIALIZED, MH_EnableHook, MH_Initialize, MH_OK,
};
use std::arch::naked_asm;
use std::ffi::{OsString, c_void};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;

const STACK_LIMIT: usize = 24;

type CheckResult = unsafe extern "C" fn(i32) -> i32;
static mut ORIGINAL: *mut c_void = null_mut();

struct Trace {
    path: PathBuf,
    image_size: usize,
}

static TRACE: Mutex<Option<Trace>> = Mutex::new(None);

/// The caller lends the expected target and a terminated path.
pub(crate) unsafe extern "C" fn install(target: usize, image_size: u32, path: *const u16) -> i32 {
    let initialized = unsafe { MH_Initialize() };
    if initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED {
        return initialized;
    }
    let created = unsafe { MH_CreateHook(target as _, detour as *mut c_void, &raw mut ORIGINAL) };
    if created != MH_OK {
        return created;
    }
    // Publish before the detour becomes reachable. Keep the trampoline and trace settings
    // after a refused enable because rollback must not free a potentially reachable detour.
    let path = PathBuf::from(OsString::from_wide(unsafe { path_units(path) }));
    *lock_or_recover(&TRACE) = Some(Trace {
        path,
        image_size: image_size as usize,
    });
    unsafe { MH_EnableHook(target as _) }
}

// Read the incoming return address before a prologue changes RSP. The tail jump keeps
// the original native frame visible to the Windows stack walker.
#[unsafe(naked)]
unsafe extern "C" fn detour(_: i32) -> i32 {
    naked_asm!("mov rdx, [rsp]", "jmp {}", sym observe);
}

unsafe extern "C" fn observe(result: i32, caller: usize) -> i32 {
    if result < 0 {
        let mut stack = [null_mut(); STACK_LIMIT];
        let count = unsafe {
            RtlCaptureStackBackTrace(1, STACK_LIMIT as u32, stack.as_mut_ptr(), null_mut())
        };
        record(
            result,
            unsafe { GetModuleHandleW(null()) } as usize,
            caller,
            unsafe { GetTickCount64() },
            unsafe { GetCurrentThreadId() },
            &stack[..usize::from(count).min(STACK_LIMIT)],
        );
    }
    // MinHook publishes the original before enabling this entry.
    let original: CheckResult = unsafe { std::mem::transmute(ORIGINAL) };
    unsafe { original(result) }
}

fn record(result: i32, base: usize, caller: usize, tick: u64, thread: u32, stack: &[*mut c_void]) {
    let trace = lock_or_recover(&TRACE);
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
