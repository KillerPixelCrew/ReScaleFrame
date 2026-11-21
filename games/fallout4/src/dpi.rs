// SPDX-License-Identifier: MIT
//! Per-monitor v2 DPI awareness before the engine creates its window or device.
//!
//! The request is made only while the renderer owner has neither and the process owns no window,
//! and is then verified for both the process and the calling thread. An existing compatible
//! awareness is accepted unchanged.
use crate::creation::{DEVICE, OWNER_RVA, WINDOW};
use crate::{Ops, Threw, image, message, read};
use rsf_game_sys::rsf_game_host_services;
use std::ffi::{CStr, c_void};
use windows_sys::Win32::Foundation::{
    ERROR_PROC_NOT_FOUND, ERROR_SUCCESS, GetLastError, HWND, LPARAM, SetLastError,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId};
use windows_sys::core::{BOOL, s, w};

type Export = unsafe extern "system" fn() -> isize;
type SetDpi = unsafe extern "system" fn(*mut c_void) -> BOOL;
type ProcessDpi = unsafe extern "system" fn(*mut c_void) -> *mut c_void;
type ThreadDpi = unsafe extern "system" fn() -> *mut c_void;
type EqualDpi = unsafe extern "system" fn(*mut c_void, *mut c_void) -> BOOL;

// DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.
const DESIRED: *mut c_void = -4isize as *mut c_void;

unsafe extern "system" fn count_process_windows(window: HWND, value: LPARAM) -> BOOL {
    let mut process = 0;
    unsafe { GetWindowThreadProcessId(window, &mut process) };
    if process == unsafe { GetCurrentProcessId() } {
        let count = value as *mut u32;
        unsafe { count.write(count.read().wrapping_add(1)) };
    }
    1
}

struct Awareness {
    set: SetDpi,
    process: ProcessDpi,
    thread: ThreadDpi,
    equal: EqualDpi,
}

impl Awareness {
    fn resolve() -> Option<Self> {
        let user32 = unsafe { GetModuleHandleW(w!("user32.dll")) };
        let export = |name| unsafe { GetProcAddress(user32, name) };
        let set = export(s!("SetProcessDpiAwarenessContext"));
        let process = export(s!("GetDpiAwarenessContextForProcess"));
        let thread = export(s!("GetThreadDpiAwarenessContext"));
        let equal = export(s!("AreDpiAwarenessContextsEqual"));
        let (Some(set), Some(process), Some(thread), Some(equal)) = (set, process, thread, equal)
        else {
            return None;
        };
        // SAFETY: the user32 exports have these signatures.
        unsafe {
            Some(Self {
                set: std::mem::transmute::<Export, SetDpi>(set),
                process: std::mem::transmute::<Export, ProcessDpi>(process),
                thread: std::mem::transmute::<Export, ThreadDpi>(thread),
                equal: std::mem::transmute::<Export, EqualDpi>(equal),
            })
        }
    }

    fn current(&self) -> (*mut c_void, *mut c_void) {
        unsafe { ((self.process)(GetCurrentProcess()), (self.thread)()) }
    }

    fn desired(&self, context: *mut c_void) -> bool {
        unsafe { (self.equal)(context, DESIRED) != 0 }
    }
}

/// The renderer owner's device and window, or None when the owner is unreadable.
fn owner() -> Option<(usize, usize)> {
    match read::<usize>(image() + OWNER_RVA)? {
        0 => Some((0, 0)),
        renderer => Some((
            read::<usize>(renderer.wrapping_add(DEVICE))?,
            read::<usize>(renderer.wrapping_add(WINDOW))?,
        )),
    }
}

/// Requests and verifies startup awareness. Returns the refusal reason, if any. The thread's
/// last error is preserved.
pub(crate) fn prepare(
    ops: &Ops,
    host: &rsf_game_host_services,
) -> Result<Option<&'static CStr>, Threw> {
    let saved_error = unsafe { GetLastError() };
    let result = request(ops, host);
    unsafe { SetLastError(saved_error) };
    result
}

fn request(ops: &Ops, host: &rsf_game_host_services) -> Result<Option<&'static CStr>, Threw> {
    let owner = owner();
    let owner_read = owner.is_some();
    let (device, window) = owner.unwrap_or((0, 0));
    let cold = owner_read && device == 0 && window == 0;
    let awareness = Awareness::resolve();
    let available = awareness.is_some();
    let null = std::ptr::null_mut();
    let (process_before, thread_before) =
        awareness.as_ref().map_or((null, null), Awareness::current);
    let prior_compatible = awareness.as_ref().is_some_and(|awareness| {
        awareness.desired(process_before) && awareness.desired(thread_before)
    });
    let mut requested = false;
    let mut accepted = false;
    let mut windows_read = false;
    let mut windows = 0u32;
    let mut error = if available {
        ERROR_SUCCESS
    } else {
        ERROR_PROC_NOT_FOUND
    };
    if let Some(awareness) = awareness.as_ref().filter(|_| cold && !prior_compatible) {
        unsafe { SetLastError(ERROR_SUCCESS) };
        let count = (&raw mut windows) as LPARAM;
        windows_read = unsafe { EnumWindows(Some(count_process_windows), count) } != 0;
        error = if windows_read {
            ERROR_SUCCESS
        } else {
            unsafe { GetLastError() }
        };
        if windows_read && windows == 0 {
            requested = true;
            unsafe { SetLastError(ERROR_SUCCESS) };
            accepted = unsafe { (awareness.set)(DESIRED) } != 0;
            error = if accepted {
                ERROR_SUCCESS
            } else {
                unsafe { GetLastError() }
            };
        }
    }
    let (process_after, thread_after) = awareness.as_ref().map_or((null, null), Awareness::current);
    let verified = cold
        && (prior_compatible || (windows_read && windows == 0))
        && awareness.as_ref().is_some_and(|awareness| {
            awareness.desired(process_after) && awareness.desired(thread_after)
        });
    if host.log.is_some() {
        let flag = |value: bool| u32::from(value);
        // Pointer fields keep the MSVC %p rendering of the previous native report.
        let text = message(format!(
            "Fallout4 DPI startup PMv2 requested={} accepted={} prior_compatible={} verified={} error={error} owner_read={} device_absent={} window_absent={} windows_read={} process_windows={windows} process_before={:016X} thread_before={:016X} process_after={:016X} thread_after={:016X}; prepare before host graphics setup.",
            flag(requested),
            flag(accepted),
            flag(prior_compatible),
            flag(verified),
            flag(owner_read),
            flag(owner_read && device == 0),
            flag(owner_read && window == 0),
            flag(windows_read),
            process_before as usize,
            thread_before as usize,
            process_after as usize,
            thread_after as usize,
        ));
        if let Ok(text) = CStr::from_bytes_with_nul(&text) {
            ops.log_host(host, text)?;
        }
    }
    Ok(if !cold {
        Some(c"Fallout4 DPI startup refused: native owner unreadable or window/device already created.")
    } else if !available {
        Some(c"Fallout4 DPI startup refused: required awareness APIs unavailable.")
    } else if !prior_compatible && (!windows_read || windows != 0) {
        Some(c"Fallout4 DPI startup refused: window enumeration failed or a process window already exists.")
    } else if !verified {
        Some(c"Fallout4 DPI startup refused: process or caller awareness is incompatible; existing thread awareness preserved.")
    } else {
        None
    })
}
