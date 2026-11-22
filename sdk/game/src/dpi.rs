// SPDX-License-Identifier: MIT
//! The verified per-monitor-v2 DPI request adapters make before the engine creates its window.
//!
//! The adapter decides when a request is safe (`cold`). The request is then made only when no
//! compatible awareness exists and the process owns no window, and is verified for both the
//! process and the calling thread. An existing compatible awareness is accepted unchanged. The
//! request changes the thread's last error.
use std::ffi::c_void;
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

/// What the request saw and did, for the adapter's report and refusal policy.
#[derive(Clone, Copy, Debug, Default)]
pub struct Startup {
    /// The four user32 awareness exports resolved.
    pub available: bool,
    /// Process and thread already had per-monitor-v2 awareness.
    pub prior_compatible: bool,
    pub windows_read: bool,
    pub process_windows: u32,
    pub requested: bool,
    pub accepted: bool,
    /// Win32 error of the last failing step, or 0.
    pub error: u32,
    /// Cold, allowed, and process and thread now have per-monitor-v2 awareness.
    pub verified: bool,
    pub process_before: usize,
    pub thread_before: usize,
    pub process_after: usize,
    pub thread_after: usize,
}

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

/// Requests per-monitor-v2 awareness when `cold` and verifies it.
pub fn request_per_monitor_v2(cold: bool) -> Startup {
    let awareness = Awareness::resolve();
    let null = std::ptr::null_mut();
    let (process_before, thread_before) =
        awareness.as_ref().map_or((null, null), Awareness::current);
    let mut startup = Startup {
        available: awareness.is_some(),
        prior_compatible: awareness.as_ref().is_some_and(|awareness| {
            awareness.desired(process_before) && awareness.desired(thread_before)
        }),
        error: if awareness.is_some() {
            ERROR_SUCCESS
        } else {
            ERROR_PROC_NOT_FOUND
        },
        process_before: process_before as usize,
        thread_before: thread_before as usize,
        ..Startup::default()
    };
    if let Some(awareness) = awareness
        .as_ref()
        .filter(|_| cold && !startup.prior_compatible)
    {
        unsafe { SetLastError(ERROR_SUCCESS) };
        let count = (&raw mut startup.process_windows) as LPARAM;
        startup.windows_read = unsafe { EnumWindows(Some(count_process_windows), count) } != 0;
        startup.error = if startup.windows_read {
            ERROR_SUCCESS
        } else {
            unsafe { GetLastError() }
        };
        if startup.windows_read && startup.process_windows == 0 {
            startup.requested = true;
            unsafe { SetLastError(ERROR_SUCCESS) };
            startup.accepted = unsafe { (awareness.set)(DESIRED) } != 0;
            startup.error = if startup.accepted {
                ERROR_SUCCESS
            } else {
                unsafe { GetLastError() }
            };
        }
    }
    let (process_after, thread_after) = awareness.as_ref().map_or((null, null), Awareness::current);
    startup.process_after = process_after as usize;
    startup.thread_after = thread_after as usize;
    startup.verified = cold
        && (startup.prior_compatible || (startup.windows_read && startup.process_windows == 0))
        && awareness.as_ref().is_some_and(|awareness| {
            awareness.desired(process_after) && awareness.desired(thread_after)
        });
    startup
}
