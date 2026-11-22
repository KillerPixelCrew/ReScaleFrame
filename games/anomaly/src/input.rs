// SPDX-License-Identifier: MIT
//! Anomaly input facts and the overlay's DirectInput gate.
//!
//! The engine samples input in `CInput::OnFrame` and polls buffered DirectInput devices from it.
//! This module delimits the polling thread and admits input facts while the native callback is
//! in flight. `input_hooks.cpp` performs the guarded frame read and invokes the original callback.
//! The buffered device policy is in `gate`; the Rust DirectInput detours are in `overlay_hooks`.

use rsf_game_sys::{RSF_GAME_INPUT_KEYBOARD, RSF_GAME_INPUT_MOUSE};
use std::cell::Cell;
#[cfg(windows)]
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(windows)]
use crate::engine_guards::{Image, NativeSite, Read};
#[cfg(windows)]
use crate::engine_slots::{Exchange, INPUT, input_counter, rsf_anomaly_slot_install};

// ---- CInput::OnFrame admission ------------------------------------------------------------------

#[cfg(windows)]
#[repr(C)]
struct InputProfile {
    sites: [NativeSite; 3],
    slot_rva: u32,
    frame_code: *const std::ffi::c_char,
}

#[cfg(windows)]
const _: () = {
    assert!(size_of::<InputProfile>() == 112);
    assert!(std::mem::offset_of!(InputProfile, slot_rva) == 96);
    assert!(std::mem::offset_of!(InputProfile, frame_code) == 104);
};

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_install(
    image: &Image,
    profile: &InputProfile,
    counter_rva: u32,
    replacement: *mut c_void,
    exchange: Exchange,
    reason: Option<&mut *const std::ffi::c_char>,
) -> bool {
    for site in &profile.sites {
        if !image.matches(site) {
            if let Some(reason) = reason {
                *reason = site.name;
            }
            return false;
        }
    }
    let frame = &profile.sites[0];
    if !unsafe { image.matches_hex_guard(frame.rva, profile.frame_code) } {
        if let Some(reason) = reason {
            *reason = c"CInput OnFrame complete body/vtable guard".as_ptr();
        }
        return false;
    }
    if !rsf_anomaly_slot_install(
        INPUT as u32,
        image,
        profile.slot_rva,
        frame.rva,
        counter_rva,
        replacement,
        exchange,
    ) {
        if let Some(reason) = reason {
            *reason = c"CInput primary slot 0".as_ptr();
        }
        return false;
    }
    true
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
thread_local! {
    // True only while this thread executes an admitted `CInput::OnFrame`.
    static POLLING: Cell<bool> = const { Cell::new(false) };
}

// Polling ticket bits carried by the detour until the original returns.
const POLLING_STARTED: u32 = 1;
const WAS_POLLING: u32 = 2;

/// Counts an `OnFrame` call in flight and admits its engine frame event.
#[cfg(any(windows, test))]
fn frame_enter() -> bool {
    IN_FLIGHT.fetch_add(1, Ordering::AcqRel);
    ACTIVE.load(Ordering::Acquire)
}

/// The detour has reported the frame boundary and is about to call the original.
#[cfg(any(windows, test))]
fn polling_begin() -> u32 {
    POLLING_STARTED
        | if POLLING.replace(true) {
            WAS_POLLING
        } else {
            0
        }
}

#[cfg(windows)]
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_frame_begin(read: Read, emit: unsafe extern "C" fn(u32)) -> u32 {
    if !frame_enter() {
        return 0;
    }
    let mut frame = 0u32;
    if !unsafe {
        read(
            (&raw mut frame).cast(),
            input_counter().cast(),
            size_of::<u32>(),
        )
    } {
        return 0;
    }
    unsafe { emit(frame) };
    polling_begin()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_frame_exit(ticket: u32) {
    if ticket & POLLING_STARTED != 0 {
        POLLING.set(ticket & WAS_POLLING != 0);
    }
    IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_input_activate(active: bool) {
    ACTIVE.store(active, Ordering::Release);
}

/// No `OnFrame` call is executing, so a retired hook can be left behind.
#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_idle() -> bool {
    IN_FLIGHT.load(Ordering::Acquire) == 0
}

/// The input kinds to report for hardware records the gate returned to the engine, or 0 when the
/// read did not happen inside an admitted `OnFrame` on this thread.
fn route(kind: u8) -> u32 {
    if !POLLING.get() || !ACTIVE.load(Ordering::Acquire) {
        return 0;
    }
    match kind {
        MOUSE => RSF_GAME_INPUT_MOUSE,
        KEYBOARD => RSF_GAME_INPUT_KEYBOARD,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_input_received(kind: u8, emit: unsafe extern "C" fn(u32, u32)) {
    let kinds = route(kind);
    if kinds != 0 {
        unsafe { emit(kinds, 0) };
    }
}

#[cfg(test)]
#[test]
fn routing_starts_after_frame_report_and_ends_at_poll_retirement() {
    rsf_anomaly_input_activate(true);
    assert!(frame_enter());
    assert!(!rsf_anomaly_input_idle());
    assert_eq!(route(KEYBOARD), 0);

    let outer = polling_begin();
    assert_eq!(route(KEYBOARD), RSF_GAME_INPUT_KEYBOARD);
    assert!(frame_enter());
    let inner = polling_begin();
    rsf_anomaly_input_frame_exit(inner);
    assert_eq!(route(MOUSE), RSF_GAME_INPUT_MOUSE);

    rsf_anomaly_input_activate(false);
    assert_eq!(route(MOUSE), 0);
    assert!(!frame_enter());
    rsf_anomaly_input_frame_exit(0);
    assert!(!rsf_anomaly_input_idle());
    rsf_anomaly_input_frame_exit(outer);
    assert!(rsf_anomaly_input_idle());
}

pub(super) const MOUSE: u8 = 0;
pub(super) const KEYBOARD: u8 = 1;

#[cfg(any(windows, test))]
pub(crate) mod gate;
