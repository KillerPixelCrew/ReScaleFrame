// SPDX-License-Identifier: MIT
//! Godot window and input ownership:
//!
//! - the input window: the first window of this process whose class is `Engine`, the class
//!   Godot's Windows display server registers. The Runtime's presentation bridge asks through
//!   the `accept_window` callback and the Vulkan engine driver through `rsfg_vk_window`; the
//!   first admitted window stays until `rsfg_shutdown`;
//! - the input session the engine module's CPU events carry, stamped on every window message;
//! - the window procedure, attached at the first `FRAME_BEGIN` after a window was admitted, which
//!   reports each message it receives as an `INPUT_EVENT` and forwards it to the procedure it
//!   replaced. Each subclassed window keeps its replaced procedure until it is restored or the
//!   window is destroyed, so a refused restore keeps forwarding and a later window is admitted
//!   independently;
//! - the Runtime CPU service bound at graphics setup, and window admission for Present.
//!
//! What an input event, a ping or a present marker means, and any pacing, stays with the Runtime:
//! this owner reports what the window received and when.
use crate::control::Status;
use crate::sys::*;
use rsf_game_sys::ffi_util::{ascii_wide, lock_or_recover};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_INPUT_EVENT,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_RENDER_PRESENT,
    RSF_GAME_RENDER_SUBMISSION, rsf_game_cpu_event, rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicPtr, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, GWLP_WNDPROC, GetClassNameW, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW, WM_NCDESTROY,
    WNDPROC,
};

type Service = rsf_cpu_session_service;

/// The window class of Godot's Windows display server.
const ENGINE_CLASS: [u16; 6] = ascii_wide(b"Engine");

#[derive(Clone, Copy, PartialEq, Eq)]
struct PresentReceipt {
    session: u64,
    source: u64,
    window: u64,
}

thread_local! {
    static PRESENT: Cell<Option<PresentReceipt>> = const { Cell::new(None) };
}

/// The admitted input window; null until one is admitted and after `release`.
static INPUT: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
/// The session of the engine module's last CPU event; zero after `release`.
static SESSION: AtomicU64 = AtomicU64::new(0);
/// Each subclassed window and the procedure `procedure` replaced in it.
static PREVIOUS: Mutex<Vec<(usize, isize)>> = Mutex::new(Vec::new());
static SERVICE: RwLock<Option<&'static Service>> = RwLock::new(None);

/// The procedure attached to the input window. It releases its lock before forwarding.
unsafe extern "system" fn procedure(
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let previous = {
        let mut procedures = lock_or_recover(&PREVIOUS);
        procedures
            .iter()
            .position(|(owner, _)| *owner == window.addr())
            .map_or(0, |index| {
                if message == WM_NCDESTROY {
                    procedures.swap_remove(index).1
                } else {
                    procedures[index].1
                }
            })
    };
    if input() == window {
        if message == WM_NCDESTROY {
            let _ = INPUT.compare_exchange(
                window,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        let (mut counter, mut frequency) = (0, 0);
        unsafe {
            QueryPerformanceCounter(&mut counter);
            QueryPerformanceFrequency(&mut frequency);
        }
        let event = rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage: RSF_GAME_CPU_INPUT_EVENT,
            session_id: SESSION.load(Ordering::Acquire),
            source_frame_id: 0,
            timestamp_qpc: counter as u64,
            qpc_frequency: frequency as u64,
            render_expected: 0,
            input_kind: 0,
            message_id: message,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags: RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE,
        };
        if let Some(report) = service().and_then(|service| service.report) {
            // SAFETY: the event is borrowed for the Runtime report.
            unsafe { report(&event) };
        }
    }
    unsafe {
        if previous == 0 {
            DefWindowProcW(window, message, wparam, lparam)
        } else {
            // CallWindowProcW interprets the previous procedure token returned by Win32.
            CallWindowProcW(
                std::mem::transmute::<isize, WNDPROC>(previous),
                window,
                message,
                wparam,
                lparam,
            )
        }
    }
}

fn procedure_address() -> isize {
    let procedure: unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize = procedure;
    procedure as usize as isize
}

fn service() -> Option<&'static Service> {
    *SERVICE.read().unwrap_or_else(|error| error.into_inner())
}

/// Binds the session's Runtime-owned CPU service, an immutable process-lived record.
pub(crate) fn bind_cpu_service(vulkan: bool) {
    let service = unsafe {
        if vulkan {
            rsf_latencyvk_cpu_service()
        } else {
            rsf_fg12_cpu_service()
        }
    };
    // SAFETY: the Runtime returns an immutable process-lived record or null.
    let service = unsafe { crate::checked(service, RSF_CPU_SESSION_SERVICE_VERSION) };
    *SERVICE.write().unwrap_or_else(|error| error.into_inner()) = service;
}

pub(crate) fn unbind_cpu_service() {
    *SERVICE.write().unwrap_or_else(|error| error.into_inner()) = None;
}

pub(crate) fn input() -> *mut c_void {
    INPUT.load(Ordering::Acquire)
}

/// Admits `window` when it belongs to this process and has the `Engine` class and no other
/// window was admitted first. True for the admitted window.
pub(crate) fn accept(window: *mut c_void) -> bool {
    if window.is_null() {
        return false;
    }
    let mut process = 0;
    let mut class = [0u16; 64];
    let length = unsafe {
        GetWindowThreadProcessId(window, &mut process);
        GetClassNameW(window, class.as_mut_ptr(), class.len() as i32)
    };
    if process != unsafe { GetCurrentProcessId() }
        || class.get(..length.max(0) as usize) != Some(&ENGINE_CLASS[..])
    {
        return false;
    }
    let _ = INPUT.compare_exchange(ptr::null_mut(), window, Ordering::AcqRel, Ordering::Acquire);
    input() == window
}

/// The foreground process and the visibility of the admitted input window.
pub(crate) fn apply_facts(status: &mut Status) {
    let mut foreground = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut foreground);
        status.foreground_process_id = foreground;
        status.window_foreground = u32::from(foreground == GetCurrentProcessId());
        status.window_visible = u32::from(IsWindowVisible(input()) != 0);
    }
}

/// `rsfg_cpu`: keeps the session, attaches the procedure at a frame begin, then reports the event
/// to the session's backend.
///
/// # Safety
/// `event` is null or a readable `rsf_game_cpu_event` of its stated size.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsfg_cpu(event: *const rsf_game_cpu_event) {
    // SAFETY: the caller's guarantee; the record is read only when its size covers it.
    let Some(event) = (unsafe { rsf_game_sys::ffi_util::checked(event) }) else {
        return;
    };
    let Some(report) = service().and_then(|service| service.report) else {
        return;
    };
    SESSION.store(event.session_id, Ordering::Release);
    if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
        attach();
    }
    // SAFETY: the event is borrowed for the Runtime report. No routing lock is held.
    unsafe { report(event) };
}

/// Replaces the admitted window's procedure once. A failed replacement leaves nothing attached,
/// and the next frame begin tries again.
fn attach() {
    let mut procedures = lock_or_recover(&PREVIOUS);
    let window = input();
    if window.is_null() || procedures.iter().any(|(owner, _)| *owner == window.addr()) {
        return;
    }
    let previous = unsafe { SetWindowLongPtrW(window, GWLP_WNDPROC, procedure_address()) };
    if previous != 0 {
        procedures.push((window.addr(), previous));
    }
}

/// The window part of `rsfg_shutdown`. The procedure is restored only while it is still the
/// window's own and the restore succeeds. A refused restore or an intervening subclass keeps
/// forwarding to the replaced procedure. The admission and session end either way.
pub(crate) fn release() {
    let mut procedures = lock_or_recover(&PREVIOUS);
    INPUT.store(ptr::null_mut(), Ordering::Release);
    SESSION.store(0, Ordering::Release);
    procedures.retain(|&(owner, previous)| {
        let window = ptr::without_provenance_mut(owner);
        unsafe {
            GetWindowLongPtrW(window, GWLP_WNDPROC) != procedure_address()
                || SetWindowLongPtrW(window, GWLP_WNDPROC, previous) == 0
        }
    });
}

/// The PCL ping for the admitted window, null when none.
#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsfg_request_latency_ping() -> c_int {
    service()
        .and_then(|service| service.ping)
        // SAFETY: the window is an opaque handle, including null for an unbound window.
        .map_or(0, |ping| unsafe { ping(input()) })
}

pub(crate) fn admits_present(window: u64) -> bool {
    window != 0 && input().addr() as u64 == window
}

/// One engine execution receipt: a submission, or a Present bracket of the admitted window that
/// must end with the receipt it began with.
pub(crate) fn execution(pass: &rsf_game_render_pass, begin: u32) -> bool {
    if pass.session_id == 0 || pass.source_frame_id == 0 || begin > 1 {
        return false;
    }
    let Some(report) = service().and_then(|service| service.execution) else {
        return false;
    };
    match pass.role {
        RSF_GAME_RENDER_SUBMISSION => {}
        RSF_GAME_RENDER_PRESENT => {
            let receipt = PresentReceipt {
                session: pass.session_id,
                source: pass.source_frame_id,
                window: pass.window_key,
            };
            if begin != 0 {
                if PRESENT.get().is_some() || !admits_present(receipt.window) {
                    return false;
                }
                PRESENT.set(Some(receipt));
            } else if PRESENT.get() == Some(receipt) {
                PRESENT.set(None);
            } else {
                return false;
            }
        }
        _ => return false,
    }
    // SAFETY: the execution fact is borrowed only for the report.
    unsafe { report(pass, begin) };
    true
}
