// SPDX-License-Identifier: MIT
//! Removed GLFW window messages. A thread-local WH_GETMESSAGE hook on the GLFW owner thread reports
//! every PM_REMOVE message for the parent, the presentation child or the thread itself. It never
//! replaces GLFW callbacks or injects input; the Runtime recognises pings and assigns frame identity.

use crate::platform_abi::rsf_minecraft_message_fn;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering::SeqCst};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetWindowThreadProcessId, MSG, PM_REMOVE, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_GETMESSAGE,
};

type Sink = unsafe extern "C" fn(message: u32);

static PARENT: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static CHILD: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
// The installed Sink, or null.
static SINK: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static HOOK: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

fn sink() -> Option<Sink> {
    let address = SINK.load(SeqCst);
    // SAFETY: SINK only ever holds null or an address stored from a Sink in install.
    (!address.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, Sink>(address) })
}

unsafe extern "system" fn observe(code: i32, removed: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0 && removed == PM_REMOVE as WPARAM {
        // SAFETY: a non-negative WH_GETMESSAGE code passes the dequeued MSG in lParam.
        let event = unsafe { &*(data as *const MSG) };
        let window: HWND = event.hwnd;
        if (window.is_null() || window == PARENT.load(SeqCst) || window == CHILD.load(SeqCst))
            && let Some(report) = sink()
        {
            // SAFETY: the sink was supplied as a live C callback by the installer.
            unsafe { report(event.message) };
        }
    }
    // SAFETY: forwarding the unchanged hook arguments.
    unsafe { CallNextHookEx(null_mut(), code, removed, data) }
}

/// Installs the hook on the calling thread, which must own parent. Refuses while installed.
pub(crate) fn install(parent: HWND, child: HWND, report: rsf_minecraft_message_fn) -> bool {
    // SAFETY: plain thread and window queries; a stale or null window returns zero.
    let (owner, current) = unsafe {
        (
            GetWindowThreadProcessId(parent, null_mut()),
            GetCurrentThreadId(),
        )
    };
    if !HOOK.load(SeqCst).is_null() || owner != current {
        return false;
    }
    PARENT.store(parent, SeqCst);
    CHILD.store(child, SeqCst);
    SINK.store(
        report.map_or(null_mut(), |report| report as *mut c_void),
        SeqCst,
    );
    // SAFETY: a thread-local hook on the current thread needs no module handle.
    let hook = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(observe), null_mut(), current) };
    HOOK.store(hook, SeqCst);
    !hook.is_null()
}

/// Stops reporting first, then removes the hook. Safe to call when nothing is installed.
pub(crate) fn close() {
    SINK.store(null_mut(), SeqCst);
    PARENT.store(null_mut(), SeqCst);
    CHILD.store(null_mut(), SeqCst);
    let hook = HOOK.swap(null_mut(), SeqCst);
    if !hook.is_null() {
        // SAFETY: hook came from SetWindowsHookExW and is removed once.
        unsafe { UnhookWindowsHookEx(hook) };
    }
}
