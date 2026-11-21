// SPDX-License-Identifier: MIT
use crate::overlay_pointer;
use crate::overlay_visibility::{self, rsf_core_overlay_toggle_key};
use crate::sr_session::has_capacity;
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
type LogFn = Option<unsafe extern "C" fn(Handle, *const c_char)>;
type InvokeLog = unsafe extern "C" fn(LogFn, Handle, *const c_char);

const ABI_VERSION: u32 = 1;
const INSERT: u32 = 0x2d;
const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const ALREADY_INSTALLED: i32 = -3;
const NOT_INSTALLED: i32 = -4;
const SUBCLASS_FAILED: i32 = -5;
const FOREIGN_SUBCLASS: i32 = -6;

#[repr(C)]
struct Options {
    struct_size: u32,
    abi_version: u32,
    toggle_virtual_key: u32,
    log: LogFn,
    log_user: Handle,
}

#[repr(C)]
struct Platform {
    install_cursor: unsafe extern "C" fn(),
    retire_cursor: unsafe extern "C" fn(u32, u32),
    log: InvokeLog,
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

    fn line(self, text: String) {
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

fn reset_input(toggle: u32, down: bool) -> bool {
    rsf_core_overlay_input_clear_holds();
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_input_install(
    window: Handle,
    options: *const Options,
    procedure: isize,
    platform: &Platform,
) -> i32 {
    if window.is_null() || !unsafe { has_capacity(options) } {
        return INVALID;
    }
    let options = unsafe { &*options };
    if options.abi_version != ABI_VERSION {
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

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_input_uninstall(
    procedure: isize,
    platform: &Platform,
) -> i32 {
    let _lifecycle = LIFECYCLE.lock().unwrap_or_else(PoisonError::into_inner);
    let window = rsf_core_overlay_input_window();
    if window.is_null() {
        return NOT_INSTALLED;
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
    unsafe { (platform.retire_cursor)(visible.into(), alive.into()) };
    set_logger(None);
    if alive && let Some(log) = log {
        log.say(c"overlay input: window procedure restored");
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_input_window() -> Handle {
    WINDOW.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_input_original() -> isize {
    ORIGINAL.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_input_log(message: *const c_char) {
    if !message.is_null()
        && let Some(log) = logger()
    {
        log.say(unsafe { CStr::from_ptr(message) });
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_input_hold(key: u32) {
    if let Some(held) = HELD.get(key as usize) {
        held.store(true, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_input_release(key: u32) -> u32 {
    HELD.get(key as usize)
        .is_some_and(|held| held.swap(false, Ordering::Relaxed))
        .into()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_overlay_input_clear_holds() {
    for held in &HELD {
        held.store(false, Ordering::Relaxed);
    }
}
