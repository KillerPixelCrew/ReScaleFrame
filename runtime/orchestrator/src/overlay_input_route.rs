// SPDX-License-Identifier: MIT
use crate::overlay_abi::rsf_overlay_input_counters as Counters;
use crate::overlay_cursor_hooks::rsf_core_overlay_cursor_ready;
use crate::overlay_pointer;
use crate::overlay_visibility::{
    self, Apply, rsf_core_overlay_toggle_key, rsf_core_overlay_visible,
};
use crate::overlay_window::{clear_holds, hold, release, rsf_core_overlay_input_window};
use crate::sr_session::has_capacity;
use std::sync::atomic::{AtomicU64, Ordering};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

const DOWN: u32 = 1;
const SYSTEM: u32 = 2;
const REPEAT: u32 = 4;

static POINTER_RECEIVED: AtomicU64 = AtomicU64::new(0);
static POINTER_CONSUMED: AtomicU64 = AtomicU64::new(0);
static BUTTON_DOWNS: AtomicU64 = AtomicU64::new(0);
static WHEEL: AtomicU64 = AtomicU64::new(0);
static RAW: AtomicU64 = AtomicU64::new(0);
static KEYS: AtomicU64 = AtomicU64::new(0);
static FRAMES: AtomicU64 = AtomicU64::new(0);
static FRAMES_WITH_BUTTON: AtomicU64 = AtomicU64::new(0);
static LAST_POSITION: AtomicU64 = AtomicU64::new(0);

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_route_key(key: u32, flags: u32, visible: u32, apply: Apply) -> u32 {
    let visible = visible != 0;
    let system = flags & SYSTEM != 0;
    let toggle = key == rsf_core_overlay_toggle_key();
    if flags & DOWN != 0 {
        if toggle {
            // The hidden panel still passes its opening press through so
            // the game's matching release must be forwarded as well.
            let now = unsafe { GetTickCount64() };
            if overlay_visibility::message_press(flags & REPEAT != 0, now, apply) && !visible {
                hold(key);
            }
            return visible.into();
        }
        if !visible || system {
            hold(key);
            return 0;
        }
    } else {
        if toggle {
            overlay_visibility::message_release();
        }
        let owed = release(key);
        // Never swallow a system release or a release for a press the game saw.
        if !visible || system || owed {
            return 0;
        }
    }
    KEYS.fetch_add(1, Ordering::Relaxed);
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_route_button(key: u32, down: u32, visible: u32) -> u32 {
    let visible = visible != 0;
    if down != 0 {
        if !visible {
            hold(key);
            return 0;
        }
        BUTTON_DOWNS.fetch_add(1, Ordering::Relaxed);
        rsf_core_overlay_note_pointer(1, 1);
        return 1;
    }
    let consumed = !release(key) && visible;
    if visible {
        rsf_core_overlay_note_pointer(1, consumed.into());
    }
    consumed.into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_route_focus_lost() {
    clear_holds();
    overlay_pointer::focus_lost();
    overlay_visibility::focus_lost();
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_note_pointer(received: u32, consumed: u32) {
    if received != 0 {
        POINTER_RECEIVED.fetch_add(received.into(), Ordering::Relaxed);
    }
    if consumed != 0 {
        POINTER_CONSUMED.fetch_add(consumed.into(), Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_note_wheel(notches: f32) {
    overlay_pointer::scroll(notches);
    rsf_core_overlay_note_pointer(1, 1);
    WHEEL.fetch_add(1, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_note_raw() {
    RAW.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn sampled(x: f32, y: f32, buttons: u32) {
    FRAMES.fetch_add(1, Ordering::Relaxed);
    if buttons != 0 {
        FRAMES_WITH_BUTTON.fetch_add(1, Ordering::Relaxed);
    }
    LAST_POSITION.store(
        (u64::from(x as u32) << 32) | u64::from(y as u32),
        Ordering::Relaxed,
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_input_counters(out: *mut Counters) -> i32 {
    if !unsafe { has_capacity(out) } {
        return -1;
    }
    let position = LAST_POSITION.load(Ordering::Relaxed);
    let record = Counters {
        struct_size: unsafe { out.cast::<u32>().read() },
        installed: (!rsf_core_overlay_input_window().is_null()).into(),
        visible: rsf_core_overlay_visible(),
        cursor_freed: rsf_core_overlay_cursor_ready(),
        pointer_received: POINTER_RECEIVED.load(Ordering::Relaxed),
        pointer_consumed: POINTER_CONSUMED.load(Ordering::Relaxed),
        button_downs_consumed: BUTTON_DOWNS.load(Ordering::Relaxed),
        wheel_consumed: WHEEL.load(Ordering::Relaxed),
        raw_mouse_reports: RAW.load(Ordering::Relaxed),
        keys_consumed: KEYS.load(Ordering::Relaxed),
        frames_visible: FRAMES.load(Ordering::Relaxed),
        frames_with_button: FRAMES_WITH_BUTTON.load(Ordering::Relaxed),
        last_pointer_x: (position >> 32) as u32,
        last_pointer_y: position as u32,
    };
    unsafe { out.write(record) };
    0
}
