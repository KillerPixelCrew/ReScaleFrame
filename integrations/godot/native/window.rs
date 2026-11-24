// SPDX-License-Identifier: MIT
//! Godot window and input ownership:
//!
//! - the input window: the first window of this process whose class is `Engine`, the class
//!   Godot's Windows display server registers. The Runtime's presentation bridge asks through
//!   the `accept_window` trampoline and the Vulkan engine driver through `rsfg_vk_window`; the
//!   first admitted window stays until `rsfg_shutdown`;
//! - the input session the engine module's CPU events carry, stamped on every window message;
//! - the window procedure, attached at the first `FRAME_BEGIN` after a window was admitted, which
//!   reports each message it receives as an `INPUT_EVENT` with the message ID and forwards it to
//!   the procedure it replaced, and its restoration at `rsfg_shutdown` only while it is still the
//!   window's procedure;
//! - the Runtime CPU service bound at graphics setup, and window admission for Present.
//!
//! `host.rs` keeps the typed Win32 and Runtime calls as leaves. What an input event, a ping
//! or a present marker means, and any pacing, stays with the Runtime (Rule 3): this owner reports
//! what the window received and when.
use crate::platform::{CpuService, Platform};
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

#[derive(Clone, Copy, PartialEq, Eq)]
struct PresentReceipt {
    session: u64,
    source: u64,
    window: u64,
}

thread_local! {
    static PRESENT: Cell<Option<PresentReceipt>> = const { Cell::new(None) };
}

/// The window class of Godot's Windows display server.
const ENGINE_CLASS: [u16; 6] = ascii_wide(b"Engine");

const WM_NCDESTROY: u32 = 0x0082;

/// The procedure attached to the input window. It releases its lock before forwarding.
///
/// # Safety
/// Called by Windows with the window's message.
unsafe extern "system" fn procedure(
    window: *mut c_void,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    let control = &*crate::CONTROL;
    control.window.message(window, message, wparam, lparam)
}

/// `WNDPROC`.
type WindowProcedure = unsafe extern "system" fn(*mut c_void, u32, usize, isize) -> isize;

/// `procedure` as `GWLP_WNDPROC` stores it.
pub(crate) fn procedure_address() -> isize {
    let procedure: WindowProcedure = procedure;
    procedure as usize as isize
}

pub(crate) struct Window {
    /// The admitted input window; null until one is admitted and after `release`.
    input: AtomicPtr<c_void>,
    /// The session of the engine module's last CPU event; zero after `release`.
    session: AtomicU64,
    /// The procedure `procedure` replaced; nonzero while it is attached.
    previous: Mutex<Vec<(usize, isize)>>,
    cpu_service: RwLock<Option<&'static CpuService>>,
}

impl Window {
    pub(crate) const fn new() -> Self {
        Self {
            input: AtomicPtr::new(ptr::null_mut()),
            session: AtomicU64::new(0),
            previous: Mutex::new(Vec::new()),
            cpu_service: RwLock::new(None),
        }
    }

    pub(crate) fn bind_cpu_service(&self, service: Option<&'static CpuService>) {
        *self
            .cpu_service
            .write()
            .unwrap_or_else(|err| err.into_inner()) = service;
    }

    pub(crate) fn cpu_service(&self) -> Option<&'static CpuService> {
        *self
            .cpu_service
            .read()
            .unwrap_or_else(|err| err.into_inner())
    }

    pub(crate) fn input(&self) -> *mut c_void {
        self.input.load(Ordering::Acquire)
    }

    /// Admits `window` when it belongs to this process and has the `Engine` class and no other
    /// window was admitted first. True for the admitted window.
    pub(crate) fn accept(&self, window: *mut c_void) -> bool {
        if window.is_null() {
            return false;
        }
        let identity = Platform.window_identity(window);
        let length = identity.class_length as usize;
        if identity.process_id != identity.current_process_id
            || identity.class_name.get(..length) != Some(&ENGINE_CLASS[..])
        {
            return false;
        }
        let _ = self.input.compare_exchange(
            ptr::null_mut(),
            window,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.input() == window
    }

    /// `rsfg_cpu` after its boot and record checks: keeps the session, attaches the procedure at a
    /// frame begin, then reports the event to the session's backend.
    pub(crate) fn cpu(&self, event: &rsf_game_cpu_event) {
        let Some(service) = self.cpu_service() else {
            return;
        };
        self.session.store(event.session_id, Ordering::Release);
        if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
            self.attach();
        }
        // SAFETY: the event is borrowed for the Runtime report. No routing lock is held.
        unsafe { (service.report)(event) };
    }

    /// Replaces the admitted window's procedure once. A failed replacement leaves nothing
    /// attached, and the next frame begin tries again.
    fn attach(&self) {
        let mut procedures = lock_or_recover(&self.previous);
        let window = self.input();
        if window.is_null() || procedures.iter().any(|(owner, _)| *owner == window.addr()) {
            return;
        }
        let previous = Platform.window_procedure_set(window, procedure_address());
        if previous != 0 {
            procedures.push((window.addr(), previous));
        }
    }

    /// One message the window received: reported as an input event of the current session, then
    /// forwarded to the replaced procedure.
    pub(crate) fn message(
        &self,
        window: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        let previous = {
            let mut procedures = lock_or_recover(&self.previous);
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
        if self.input() != window {
            return Platform.window_forward(previous, window, message, wparam, lparam);
        }
        if message == WM_NCDESTROY {
            let _ = self.input.compare_exchange(
                window,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        let (counter, frequency) = Platform.query_counter();
        let event = rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage: RSF_GAME_CPU_INPUT_EVENT,
            session_id: self.session.load(Ordering::Acquire),
            source_frame_id: 0,
            timestamp_qpc: counter,
            qpc_frequency: frequency,
            render_expected: 0,
            input_kind: 0,
            message_id: message,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags: RSF_GAME_CPU_INPUT_KEYED | RSF_GAME_CPU_WINDOW_MESSAGE,
        };
        if let Some(service) = self.cpu_service() {
            // SAFETY: the event is borrowed for the Runtime report.
            unsafe { (service.report)(&event) };
        }
        Platform.window_forward(previous, window, message, wparam, lparam)
    }

    /// The window part of `rsfg_shutdown`. The procedure is restored only while it is still the
    /// window's own and the restore succeeds. A refused restore or an intervening subclass
    /// keeps forwarding to the replaced procedure. The admission and session end either way.
    pub(crate) fn release(&self) {
        let mut procedures = lock_or_recover(&self.previous);
        self.input.store(ptr::null_mut(), Ordering::Release);
        self.session.store(0, Ordering::Release);
        procedures.retain(|&(owner, previous)| {
            let window = ptr::without_provenance_mut(owner);
            Platform.window_procedure_get(window) != procedure_address()
                || Platform.window_procedure_set(window, previous) == 0
        });
    }

    /// `rsfg_request_latency_ping`: the PCL ping for the admitted window, null when none.
    pub(crate) fn latency_ping(&self) -> c_int {
        self.cpu_service().map_or(0, |service| {
            // SAFETY: the window is an opaque handle, including null for an unbound window.
            unsafe { (service.ping)(self.input()) }
        })
    }

    pub(crate) fn admits_present(&self, window: u64) -> bool {
        window != 0 && self.input().addr() as u64 == window
    }

    pub(crate) fn execution(&self, pass: &rsf_game_render_pass, begin: u32) -> bool {
        if pass.session_id == 0 || pass.source_frame_id == 0 || begin > 1 {
            return false;
        }
        let Some(report) = self.cpu_service().and_then(|service| service.execution) else {
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
                    if PRESENT.get().is_some() || !self.admits_present(receipt.window) {
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

    /// `rsfg_vk_present_marker`: only the booted Vulkan session's admitted window marks its
    /// Present. True when the marker was reported.
    pub(crate) fn present_marker(&self, vulkan: bool, window: u64, begin: u32) -> bool {
        if !vulkan || !self.admits_present(window) {
            return false;
        }
        Platform.vulkan_present_marker(begin);
        true
    }
}

#[cfg(test)]
mod tests;
