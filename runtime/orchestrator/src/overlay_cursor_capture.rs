// SPDX-License-Identifier: MIT
use crate::overlay_cursor_hooks::rsf_core_overlay_cursor_ready;
use crate::overlay_input::{genuine_clip, genuine_position, genuine_show, set_arrow};
use crate::overlay_visibility::rsf_core_overlay_visible;
use crate::overlay_window::diagnostic;
use std::cell::Cell;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Foundation::{POINT, RECT};

const SHOW_LIMIT: u32 = 64;

struct Capture {
    last_clip: Option<RECT>,
    frozen: Option<POINT>,
    shows: u32,
}

impl Capture {
    const fn new() -> Self {
        Self {
            last_clip: None,
            frozen: None,
            shows: 0,
        }
    }
}

static CAPTURE: Mutex<Capture> = Mutex::new(Capture::new());
static SWALLOWED_WARPS: AtomicU32 = AtomicU32::new(0);
static SWALLOWED_HIDES: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static GAME_COUNT: Cell<i32> = const { Cell::new(0) };
}

fn lock() -> MutexGuard<'static, Capture> {
    crate::sync::lock(&CAPTURE)
}

fn captured() -> bool {
    rsf_core_overlay_cursor_ready() != 0 && rsf_core_overlay_visible() != 0
}

pub(super) extern "C" fn rsf_core_overlay_cursor_captured() -> u32 {
    captured().into()
}

pub(super) extern "C" fn rsf_core_overlay_cursor_swallow_warp() -> u32 {
    if captured() {
        SWALLOWED_WARPS.fetch_add(1, Ordering::Relaxed);
        1
    } else {
        0
    }
}

pub(super) unsafe extern "C" fn rsf_core_overlay_cursor_clip_request(rect: *const RECT) -> u32 {
    lock().last_clip = unsafe { rect.as_ref() }.copied();
    captured().into()
}

pub(super) unsafe extern "C" fn rsf_core_overlay_cursor_frozen(out: *mut POINT) -> u32 {
    if out.is_null() || !captured() {
        return 0;
    }
    let Some(point) = lock().frozen else {
        return 0;
    };
    unsafe { out.write(point) };
    1
}

pub(super) extern "C" fn rsf_core_overlay_cursor_show(visible: i32) -> i32 {
    if !captured() {
        let count = unsafe { genuine_show(visible) };
        GAME_COUNT.with(|value| value.set(count));
        return count;
    }
    // The game can loop until ShowCursor returns a negative count. Keep
    // its logical count separate from the panel's real cursor.
    GAME_COUNT.with(|value| {
        let count = if visible != 0 {
            value.get().saturating_add(1)
        } else {
            SWALLOWED_HIDES.fetch_add(1, Ordering::Relaxed);
            value.get().saturating_sub(1)
        };
        value.set(count);
        count
    })
}

fn open() {
    let mut point = POINT::default();
    let present = unsafe { genuine_position(&mut point) } != 0;
    lock().frozen = present.then_some(point);
    unsafe { genuine_clip(ptr::null()) };
    let mut shows = 0;
    while shows < SHOW_LIMIT {
        shows += 1;
        if unsafe { genuine_show(1) } >= 0 {
            break;
        }
    }
    lock().shows = shows;
    set_arrow();
    SWALLOWED_WARPS.store(0, Ordering::Relaxed);
    SWALLOWED_HIDES.store(0, Ordering::Relaxed);
    let suffix = if shows == 1 { "" } else { "s" };
    diagnostic(&format!(
        "overlay input: cursor freed for the panel, shown after {shows} call{suffix}"
    ));
}

fn close(restore_clip: bool) {
    let (shows, clip) = {
        let mut state = lock();
        state.frozen = None;
        (mem::take(&mut state.shows), state.last_clip)
    };
    for _ in 0..shows {
        unsafe { genuine_show(0) };
    }
    if restore_clip && let Some(clip) = clip {
        unsafe { genuine_clip(&clip) };
    }
    let warps = SWALLOWED_WARPS.load(Ordering::Relaxed);
    let hides = SWALLOWED_HIDES.load(Ordering::Relaxed);
    diagnostic(&format!(
        "overlay input: cursor returned to the game; swallowed {warps} warps and {hides} hides while the panel was open"
    ));
}

pub(super) fn rsf_core_overlay_cursor_capture(visible: u32, restore_clip: u32) {
    if rsf_core_overlay_cursor_ready() == 0 {
        return;
    }
    if visible != 0 {
        open();
    } else {
        close(restore_clip != 0);
    }
}

pub(super) extern "C" fn rsf_core_overlay_cursor_reset_capture() {
    *lock() = Capture::new();
}
