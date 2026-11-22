// SPDX-License-Identifier: MIT
use crate::wide::terminated;
use std::ffi::{CStr, c_char, c_void};
use std::slice;
use windows_sys::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, GetPrivateProfileStringW, WritePrivateProfileStringW,
};
use windows_sys::core::{PCWSTR, w};

const SECTION: PCWSTR = w!("Anomaly");

// The caller provides a null pointer or a path readable through its terminator.
pub(crate) unsafe fn path<'a>(text: PCWSTR) -> Option<&'a [u16]> {
    let (units, terminated) = unsafe { terminated(text) }?;
    (!units.is_empty()).then_some(terminated)
}

// All string pointers borrow terminated UTF-16 for the duration of the call.
pub(crate) unsafe fn read_int(path: PCWSTR, key: PCWSTR, fallback: i32) -> i32 {
    unsafe { GetPrivateProfileIntW(SECTION, key, fallback, path) as i32 }
}

pub(crate) unsafe fn read_text(
    path: PCWSTR,
    key: PCWSTR,
    fallback: PCWSTR,
    out: &mut [u16],
) -> usize {
    unsafe {
        GetPrivateProfileStringW(
            SECTION,
            key,
            fallback,
            out.as_mut_ptr(),
            out.len() as u32,
            path,
        ) as usize
    }
}

pub(crate) unsafe fn write_int(path: PCWSTR, key: PCWSTR, value: i32) -> bool {
    let value = rsf_game_sys::ffi_util::wide(&value.to_string());
    unsafe { write_text(path, key, value.as_ptr()) }
}

pub(crate) unsafe fn write_text(path: PCWSTR, key: PCWSTR, value: PCWSTR) -> bool {
    unsafe { WritePrivateProfileStringW(SECTION, key, value, path) != 0 }
}

// The SR platform table supplies narrow keys and defaults. Preserve its
// original byte-wise widening at the boundary.
pub(crate) unsafe fn widen(text: *const c_char) -> Vec<u16> {
    let bytes = if text.is_null() {
        &[]
    } else {
        unsafe { CStr::from_ptr(text) }.to_bytes()
    };
    bytes.iter().copied().map(u16::from).chain([0]).collect()
}

pub(crate) unsafe extern "C" fn rsf_anomaly_sr_ini_int(
    path: *const c_void,
    key: *const c_char,
    fallback: i32,
) -> i32 {
    let key = unsafe { widen(key) };
    unsafe { read_int(path.cast(), key.as_ptr(), fallback) }
}

pub(crate) unsafe extern "C" fn rsf_anomaly_sr_ini_text(
    path: *const c_void,
    key: *const c_char,
    fallback: *const c_char,
    out: *mut u16,
    capacity: u32,
) -> u32 {
    if out.is_null() || capacity == 0 {
        return 0;
    }
    let key = unsafe { widen(key) };
    let fallback = unsafe { widen(fallback) };
    let out = unsafe { slice::from_raw_parts_mut(out, capacity as usize) };
    unsafe { read_text(path.cast(), key.as_ptr(), fallback.as_ptr(), out) as u32 }
}
