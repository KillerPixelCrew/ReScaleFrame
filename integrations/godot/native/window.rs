// SPDX-License-Identifier: MIT
//! Godot window and input ownership formerly held in `runtime.cpp` globals:
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
//! - the backend routing of CPU events and PCL pings (the Vulkan latency owner while the Vulkan
//!   session is booted, otherwise the D3D12 FG owner, as before) and the Vulkan present-marker
//!   admission (only the admitted window's Present).
//!
//! `runtime.cpp` keeps the typed Win32 and Runtime calls as leaves. What an input event, a ping
//! or a present marker means, and any pacing, stays with the Runtime (Rule 3): this owner reports
//! what the window received and when.
use crate::platform::{self, Platform};
use rsf_game_sys::{
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_INPUT_EVENT,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_WINDOW_MESSAGE, rsf_game_cpu_event,
};
use std::ffi::{c_int, c_void};
use std::ptr;
use std::sync::atomic::{AtomicIsize, AtomicPtr, AtomicU64, Ordering};

const fn wide<const N: usize>(text: &[u8; N]) -> [u16; N] {
    let mut out = [0; N];
    let mut i = 0;
    while i < N {
        out[i] = text[i] as u16;
        i += 1;
    }
    out
}

/// The window class of Godot's Windows display server.
const ENGINE_CLASS: [u16; 6] = wide(b"Engine");

/// The procedure attached to the input window. It holds no lock, so the procedures it forwards to
/// may re-enter it.
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
    match platform::registered() {
        Some(platform) => control.window.message(
            platform,
            control.vulkan.booted(),
            window,
            message,
            wparam,
            lparam,
        ),
        // Not reached: the procedure is attached only through a registered table.
        None => 0,
    }
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
    previous: AtomicIsize,
}

impl Window {
    pub(crate) const fn new() -> Self {
        Self {
            input: AtomicPtr::new(ptr::null_mut()),
            session: AtomicU64::new(0),
            previous: AtomicIsize::new(0),
        }
    }

    pub(crate) fn input(&self) -> *mut c_void {
        self.input.load(Ordering::Acquire)
    }

    /// Admits `window` when it belongs to this process and has the `Engine` class and no other
    /// window was admitted first. True for the admitted window.
    pub(crate) fn accept(&self, platform: &Platform, window: *mut c_void) -> bool {
        if window.is_null() {
            return false;
        }
        let identity = platform.window_identity(window);
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
    pub(crate) fn cpu(&self, platform: &Platform, vulkan: bool, event: &rsf_game_cpu_event) {
        self.session.store(event.session_id, Ordering::Release);
        if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
            self.attach(platform);
        }
        platform.cpu_event(vulkan, event);
    }

    /// Replaces the admitted window's procedure once. A failed replacement leaves nothing
    /// attached, and the next frame begin tries again.
    fn attach(&self, platform: &Platform) {
        let window = self.input();
        if window.is_null() || self.previous.load(Ordering::Acquire) != 0 {
            return;
        }
        let previous = platform.window_procedure_set(window, procedure_address());
        self.previous.store(previous, Ordering::Release);
    }

    /// One message the window received: reported as an input event of the current session, then
    /// forwarded to the replaced procedure.
    pub(crate) fn message(
        &self,
        platform: &Platform,
        vulkan: bool,
        window: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        let previous = self.previous.load(Ordering::Acquire);
        let (counter, frequency) = platform.query_counter();
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
        platform.cpu_event(vulkan, &event);
        platform.window_forward(previous, window, message, wparam, lparam)
    }

    /// The window part of `rsfg_shutdown`. The procedure is restored only while it is still the
    /// window's own and the restore succeeds. A refused restore or an intervening subclass
    /// keeps forwarding to the replaced procedure. The admission and session end either way.
    pub(crate) fn release(&self, platform: &Platform) {
        let window = self.input();
        let previous = self.previous.load(Ordering::Acquire);
        if !window.is_null()
            && previous != 0
            && platform.window_procedure_get(window) == procedure_address()
            && platform.window_procedure_set(window, previous) != 0
        {
            self.previous.store(0, Ordering::Release);
        }
        self.input.store(ptr::null_mut(), Ordering::Release);
        self.session.store(0, Ordering::Release);
    }

    /// `rsfg_request_latency_ping`: the PCL ping for the admitted window, null when none.
    pub(crate) fn latency_ping(&self, platform: &Platform, vulkan: bool) -> c_int {
        platform.latency_ping(vulkan, self.input())
    }

    /// `rsfg_vk_present_marker`: only the booted Vulkan session's admitted window marks its
    /// Present. True when the marker was reported.
    pub(crate) fn present_marker(
        &self,
        platform: &Platform,
        vulkan: bool,
        window: u64,
        begin: u32,
    ) -> bool {
        if !vulkan || window == 0 || self.input().addr() as u64 != window {
            return false;
        }
        platform.vulkan_present_marker(begin);
        true
    }
}

#[cfg(test)]
mod tests;
