// SPDX-License-Identifier: MIT
#[cfg(feature = "native-entry")]
mod panel;

use crate::{ini, overlay, overlay::rsf_anomaly_overlay_option};
use std::ffi::c_char;
use std::ops::RangeInclusive;
use std::sync::OnceLock;
use windows_sys::core::PCWSTR;

#[cfg(feature = "native-entry")]
pub(crate) use panel::register;

// The bootstrap supplies a validated, NUL-terminated path once per process.
static PATH: OnceLock<Vec<u16>> = OnceLock::new();

fn write(key: PCWSTR, value: i32) -> bool {
    PATH.get()
        .is_some_and(|path| unsafe { ini::write_int(path.as_ptr(), key, value) })
}

/// The label must remain readable through the lifetime of the panel entry.
pub(crate) unsafe fn register_option(
    label: *const c_char,
    range: RangeInclusive<i32>,
    restart: bool,
    get: unsafe extern "C" fn() -> i32,
    set: unsafe extern "C" fn(i32) -> bool,
) {
    let option = rsf_anomaly_overlay_option {
        label,
        toggle: (range == (0..=1)).into(),
        restart: restart.into(),
        minimum: *range.start(),
        maximum: *range.end(),
        step: 1,
        divisor: 1,
        get: Some(get),
        set: Some(set),
    };
    unsafe { overlay::rsf_anomaly_overlay_option_add(&option) };
}

pub(crate) unsafe extern "C" fn rsf_anomaly_sr_persist_int(key: *const c_char, value: i32) -> bool {
    let key = unsafe { ini::widen(key) };
    write(key.as_ptr(), value)
}

pub(crate) unsafe extern "C" fn rsf_anomaly_sr_persist_text(
    key: *const c_char,
    value: *const c_char,
) -> bool {
    PATH.get().is_some_and(|path| {
        let key = unsafe { ini::widen(key) };
        let value = unsafe { ini::widen(value) };
        unsafe { ini::write_text(path.as_ptr(), key.as_ptr(), value.as_ptr()) }
    })
}
