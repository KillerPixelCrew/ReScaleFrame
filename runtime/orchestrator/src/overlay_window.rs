// SPDX-License-Identifier: MIT
use crate::overlay_host_abi::{
    RSF_OVERLAY_INPUT_ABI_VERSION, rsf_overlay_input_log_fn as LogFn,
    rsf_overlay_input_options as Options,
};
use crate::overlay_visibility::{self, rsf_core_overlay_toggle_key};
use crate::sr_session::has_capacity;
use crate::{overlay_cursor_hooks, overlay_pointer};
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, Ordering};
use std::sync::{Mutex, PoisonError};
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError},
    UI::{
        Input::KeyboardAndMouse::GetAsyncKeyState,
        WindowsAndMessaging::{
            GWLP_WNDPROC, GetWindowLongPtrW, IsWindow, IsWindowUnicode, SetWindowLongPtrW,
        },
    },
};

type Handle = *mut c_void;
type InvokeLog = unsafe extern "C" fn(LogFn, Handle, *const c_char);

const INSERT: u32 = 0x2d;
pub(super) const OK: i32 = 0;
pub(super) const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const ALREADY_INSTALLED: i32 = -3;
pub(super) const NOT_INSTALLED: i32 = -4;
const SUBCLASS_FAILED: i32 = -5;
const FOREIGN_SUBCLASS: i32 = -6;

#[repr(C)]
pub(super) struct Platform {
    pub(super) install_cursor: unsafe extern "C" fn(),
    pub(super) retire_cursor: unsafe extern "C" fn(u32, u32) -> i32,
    pub(super) log: InvokeLog,
}

const _: () = {
    assert!(size_of::<Options>() == 32);
    assert!(size_of::<Platform>() == 3 * size_of::<Handle>());
};

#[derive(Clone, Copy)]
struct Logger {
    callback: LogFn,
    user: Handle,
    invoke: InvokeLog,
}

// The user context is a token owned by the caller for the attachment lifetime.
unsafe impl Send for Logger {}

impl Logger {
    fn say(self, text: &CStr) {
        if self.callback.is_some() {
            unsafe { (self.invoke)(self.callback, self.user, text.as_ptr()) };
        }
    }

    fn line(self, text: impl Into<Vec<u8>>) {
        if self.callback.is_some()
            && let Ok(text) = CString::new(text)
        {
            self.say(&text);
        }
    }
}

static LIFECYCLE: Mutex<()> = Mutex::new(());
static WINDOW: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);
static LOGGER: Mutex<Option<Logger>> = Mutex::new(None);
static HELD: [AtomicBool; 256] = [const { AtomicBool::new(false) }; 256];

fn set_logger(logger: Option<Logger>) {
    *LOGGER.lock().unwrap_or_else(PoisonError::into_inner) = logger;
}

fn logger() -> Option<Logger> {
    *LOGGER.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) fn diagnostic(text: &str) {
    if let Some(log) = logger() {
        log.line(text);
    }
}

fn reset_input(toggle: u32, down: bool) -> bool {
    clear_holds();
    overlay_pointer::reset();
    overlay_visibility::reset(toggle, down)
}

fn swap_procedure(window: Handle, procedure: isize) -> Result<isize, u32> {
    let previous = unsafe {
        SetLastError(0);
        SetWindowLongPtrW(window, GWLP_WNDPROC, procedure)
    };
    if previous == 0 {
        let error = unsafe { GetLastError() };
        if error != 0 {
            return Err(error);
        }
    }
    Ok(previous)
}

pub(super) unsafe extern "C" fn rsf_core_overlay_input_install(
    window: Handle,
    options: *const Options,
    procedure: isize,
    platform: &Platform,
) -> i32 {
    if window.is_null() || !unsafe { has_capacity(options) } {
        return INVALID;
    }
    let options = unsafe { &*options };
    if options.abi_version != RSF_OVERLAY_INPUT_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if unsafe { IsWindow(window) } == 0 {
        return INVALID;
    }
    // The window procedure reads its forwarding target without this lock.
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(PoisonError::into_inner);
    if !rsf_core_overlay_input_window().is_null() {
        return ALREADY_INSTALLED;
    }
    let log = Logger {
        callback: options.log,
        user: options.log_user,
        invoke: platform.log,
    };
    if unsafe { IsWindowUnicode(window) } == 0 {
        log.say(c"overlay input: window is ANSI, subclassing it with the wide entry point converts it to Unicode");
    }
    let existing = unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) };
    if existing == 0 {
        let error = unsafe { GetLastError() };
        log.line(format!(
            "overlay input: could not read the window procedure, error {error}"
        ));
        return SUBCLASS_FAILED;
    }
    if existing == procedure {
        log.say(c"overlay input: the window already has this subclass procedure");
        return SUBCLASS_FAILED;
    }
    let toggle = if options.toggle_virtual_key == 0 {
        INSERT
    } else {
        options.toggle_virtual_key
    };
    let down = unsafe { GetAsyncKeyState(toggle as i32) } < 0;
    reset_input(toggle, down);
    set_logger(Some(log));
    // Publish the original before Windows can dispatch through the subclass.
    let last_original = ORIGINAL.swap(existing, Ordering::AcqRel);
    WINDOW.store(window, Ordering::Release);
    log.line(format!(
        "overlay input: subclassing window {window:p}, toggle key 0x{toggle:02x}"
    ));
    match swap_procedure(window, procedure) {
        Err(error) => {
            WINDOW.store(null_mut(), Ordering::Release);
            ORIGINAL.store(last_original, Ordering::Release);
            set_logger(None);
            log.line(format!("overlay input: subclass failed, error {error}"));
            return SUBCLASS_FAILED;
        }
        Ok(previous) if previous != 0 && previous != existing && previous != procedure => {
            ORIGINAL.store(previous, Ordering::Release);
            log.say(c"overlay input: window procedure changed during install, forwarding to the displaced one");
        }
        Ok(_) => {}
    }
    unsafe { (platform.install_cursor)() };
    OK
}

pub(super) unsafe extern "C" fn rsf_core_overlay_input_uninstall(
    procedure: isize,
    platform: &Platform,
) -> i32 {
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(PoisonError::into_inner);
    let window = rsf_core_overlay_input_window();
    if window.is_null() {
        return if overlay_cursor_hooks::has_hooks() {
            if unsafe { (platform.retire_cursor)(0, 0) } != 0 {
                OK
            } else {
                SUBCLASS_FAILED
            }
        } else {
            NOT_INSTALLED
        };
    }
    let log = logger();
    let alive = unsafe { IsWindow(window) } != 0;
    if alive {
        if unsafe { GetWindowLongPtrW(window, GWLP_WNDPROC) } != procedure {
            if let Some(log) = log {
                log.say(c"overlay input: uninstall refused, the window procedure is not ours");
            }
            return FOREIGN_SUBCLASS;
        }
        if let Err(error) = swap_procedure(window, rsf_core_overlay_input_original()) {
            if let Some(log) = log {
                log.line(format!(
                    "overlay input: could not restore the window procedure, error {error}"
                ));
            }
            return SUBCLASS_FAILED;
        }
    } else if let Some(log) = log {
        log.say(c"overlay input: window is gone, dropping the subclass without restoring");
    }
    // A message already inside the subclass can still read ORIGINAL.
    WINDOW.store(null_mut(), Ordering::Release);
    let visible = reset_input(rsf_core_overlay_toggle_key(), false);
    let retired = unsafe { (platform.retire_cursor)(visible.into(), alive.into()) } != 0;
    set_logger(None);
    if !retired {
        if let Some(log) = log {
            log.say(c"overlay input: cursor hook removal is pending");
        }
        return SUBCLASS_FAILED;
    }
    if alive && let Some(log) = log {
        log.say(c"overlay input: window procedure restored");
    }
    OK
}

pub(super) extern "C" fn rsf_core_overlay_input_window() -> Handle {
    WINDOW.load(Ordering::Acquire)
}

pub(super) extern "C" fn rsf_core_overlay_input_original() -> isize {
    ORIGINAL.load(Ordering::Acquire)
}

pub(super) fn hold(key: u32) {
    if let Some(held) = HELD.get(key as usize) {
        held.store(true, Ordering::Relaxed);
    }
}

pub(super) fn release(key: u32) -> bool {
    HELD.get(key as usize)
        .is_some_and(|held| held.swap(false, Ordering::Relaxed))
}

pub(super) fn clear_holds() {
    for held in &HELD {
        held.store(false, Ordering::Relaxed);
    }
}
