// SPDX-License-Identifier: MIT
use super::frame_capture_abi::{rsf_capture_calls, rsf_capture_connect, rsf_capture_result};
use std::ffi::c_char;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, MAX_PATH},
    Globalization::{CP_UTF8, MultiByteToWideChar},
    System::LibraryLoader::LoadLibraryW,
};

const OK: rsf_capture_result = 0;
const INVALID_ARGUMENT: rsf_capture_result = -1;
const LIBRARY_MISSING: rsf_capture_result = -2;
const API_MISSING: rsf_capture_result = -3;
const NOT_READY: rsf_capture_result = -4;

static CALLS: OnceLock<rsf_capture_calls> = OnceLock::new();
static CONNECTING: Mutex<()> = Mutex::new(());

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_capture_initialise_impl(
    library_path_utf8: *const c_char,
    output_prefix_utf8: *const c_char,
    connect: rsf_capture_connect,
) -> rsf_capture_result {
    let Some(connect) = connect else {
        return INVALID_ARGUMENT;
    };
    if library_path_utf8.is_null() {
        return INVALID_ARGUMENT;
    }
    if CALLS.get().is_some() {
        return OK;
    }
    // A library callback can reenter while initialization is in progress.
    let Ok(_connecting) = CONNECTING.try_lock() else {
        return if CALLS.get().is_some() { OK } else { NOT_READY };
    };
    if CALLS.get().is_some() {
        return OK;
    }

    let mut path = [0u16; MAX_PATH as usize * 2];
    if unsafe {
        MultiByteToWideChar(
            CP_UTF8,
            0,
            library_path_utf8.cast(),
            -1,
            path.as_mut_ptr(),
            path.len() as i32,
        )
    } == 0
    {
        return INVALID_ARGUMENT;
    }
    let library = unsafe { LoadLibraryW(path.as_ptr()) };
    if library.is_null() {
        return LIBRARY_MISSING;
    }
    let mut calls = rsf_capture_calls {
        trigger: None,
        trigger_multiple: None,
        count: None,
    };
    if unsafe { connect(library, output_prefix_utf8, &mut calls) } == 0 {
        unsafe { FreeLibrary(library) };
        return API_MISSING;
    }
    // Retain the loaded DLL for the lifetime of the published function pointers.
    let _ = CALLS.set(calls);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_capture_trigger(frames: u32) -> rsf_capture_result {
    let Some(calls) = CALLS.get() else {
        return NOT_READY;
    };
    match (frames, calls.trigger, calls.trigger_multiple) {
        (0..=1, Some(trigger), _) => unsafe { trigger() },
        (2.., _, Some(trigger_multiple)) => unsafe { trigger_multiple(frames) },
        _ => return API_MISSING,
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_capture_count() -> u32 {
    CALLS
        .get()
        .and_then(|calls| calls.count)
        .map_or(0, |count| unsafe { count() })
}
