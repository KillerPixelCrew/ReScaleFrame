// SPDX-License-Identifier: MIT
use super::{STAGE, rsf_anomaly_fg_stage_input_received};
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows_sys::Win32::UI::Shell::{
    DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const SUBCLASS_ID: usize = 0x52534642; // RSFB
static CORRECTIONS: AtomicU32 = AtomicU32::new(0);
thread_local! {
    static CORRECTING: Cell<bool> = const { Cell::new(false) };
}

pub(super) unsafe extern "C" fn accept_window(_: *mut c_void, window: HWND) -> i32 {
    if !crate::overlay::engine_window(window) {
        return 0;
    }
    // This callback is on the window's owning engine thread.
    let mut reference = 0;
    if unsafe { GetWindowSubclass(window, Some(subclass), SUBCLASS_ID, &mut reference) } == 0
        && unsafe { SetWindowSubclass(window, Some(subclass), SUBCLASS_ID, 0) } != 0
        && let Some(stage) = STAGE.get()
    {
        stage
            .log
            .text(c"Anomaly window: borderless placement guard installed on the engine window.");
    }
    1
}

unsafe extern "system" fn subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    // The Runtime recognizes its own PCL message. The adapter reports actual delivered message IDs.
    crate::native_observer::observe_window(|| rsf_anomaly_fg_stage_input_received(0, message));
    if message == WM_NCDESTROY {
        unsafe { RemoveWindowSubclass(window, Some(subclass), SUBCLASS_ID) };
    }
    let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
    if matches!(message, WM_DWMNCRENDERINGCHANGED | WM_WINDOWPOSCHANGED) {
        CORRECTING.with(|correcting| {
            if correcting.get() {
                return;
            }
            let Some((origin, bounds)) = off_origin(window) else {
                return;
            };
            correcting.set(true);
            let moved = unsafe {
                SetWindowPos(window, ptr::null_mut(), origin.left, origin.top, 0, 0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE)
            };
            correcting.set(false);
            if CORRECTIONS.fetch_add(1, Ordering::Relaxed) < 8
                && let Some(stage) = STAGE.get()
            {
                let cause = if message == WM_DWMNCRENDERINGCHANGED {
                    "DWM non-client rendering change"
                } else {
                    "position change"
                };
                stage.log.say(format!(
                    "Anomaly window: borderless window moved to {},{} ({cause}); restored to its monitor origin {},{}, moved={}.",
                    bounds.left, bounds.top, origin.left, origin.top, u32::from(moved != 0)
                ));
            }
        });
    }
    result
}

// Flip-model non-client transitions can restore the default window position after X-Ray placed
// its borderless popup. Correct only a monitor-sized popup with no caption.
fn off_origin(window: HWND) -> Option<(RECT, RECT)> {
    let style = unsafe { GetWindowLongPtrW(window, GWL_STYLE) } as u32;
    if style & WS_POPUP == 0
        || style & WS_CAPTION == WS_CAPTION
        || unsafe { IsIconic(window) != 0 || IsWindowVisible(window) == 0 }
    {
        return None;
    }
    let mut monitor = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mut rect = RECT::default();
    let ok = unsafe {
        GetWindowRect(window, &mut rect) != 0
            && GetMonitorInfoW(
                MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) != 0
    };
    let m = monitor.rcMonitor;
    (ok && rect.right - rect.left == m.right - m.left
        && rect.bottom - rect.top == m.bottom - m.top
        && (rect.left != m.left || rect.top != m.top))
        .then_some((m, rect))
}
