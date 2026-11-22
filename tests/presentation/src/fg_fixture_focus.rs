// SPDX-License-Identifier: MIT
//! The opt-in MFG focus check restores the prior window while it still owns focus.
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, GetForegroundWindow, SW_HIDE, SW_SHOW, SetForegroundWindow, ShowWindow,
};

const LIMIT: Duration = Duration::from_secs(20);

struct Focus {
    previous: HWND,
    window: HWND,
    started: Instant,
    stop: Arc<AtomicBool>,
    watchdog: Option<JoinHandle<()>>,
}

fn restore(previous: HWND, window: HWND) {
    if !previous.0.is_null() && unsafe { GetForegroundWindow() } == window {
        let _ = unsafe { SetForegroundWindow(previous) };
    }
}

impl Drop for Focus {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        restore(self.previous, self.window);
        let _ = unsafe { DestroyWindow(self.window) };
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg_bridge_fixture_focus_start(window: HWND) -> *mut c_void {
    let mut focus = Focus {
        previous: unsafe { GetForegroundWindow() },
        window,
        started: Instant::now(),
        stop: Arc::new(AtomicBool::new(false)),
        watchdog: None,
    };
    let _ = unsafe { ShowWindow(window, SW_SHOW) };
    if !unsafe { SetForegroundWindow(window) }.as_bool()
        || unsafe { GetForegroundWindow() } != window
    {
        println!("FAIL: the explicitly authorized focus request was refused by Windows");
        return null_mut();
    }

    let stop = focus.stop.clone();
    let (previous, window) = (focus.previous.0 as usize, window.0 as usize);
    let watchdog = thread::Builder::new().spawn(move || {
        let (previous, window) = (HWND(previous as *mut _), HWND(window as *mut _));
        let deadline = Instant::now() + LIMIT;
        while !stop.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if !stop.load(Ordering::Acquire) {
            restore(previous, window);
            let _ = unsafe { ShowWindow(window, SW_HIDE) };
            eprintln!("FAIL: 20-second focus deadline reached; stopping this fixture");
            let _ = unsafe { TerminateProcess(GetCurrentProcess(), 2) };
        }
    });
    let Ok(watchdog) = watchdog else {
        return null_mut();
    };
    focus.watchdog = Some(watchdog);
    Box::into_raw(Box::new(focus)).cast()
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_fg_bridge_fixture_focus_valid(lease: *const c_void) -> i32 {
    let lease = unsafe { &*lease.cast::<Focus>() };
    i32::from(lease.started.elapsed() < LIMIT && unsafe { GetForegroundWindow() } == lease.window)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fg_bridge_fixture_focus_end(lease: *mut c_void) {
    if !lease.is_null() {
        drop(unsafe { Box::from_raw(lease.cast::<Focus>()) });
    }
}
