// SPDX-License-Identifier: MIT
//! Profile arguments shared by the native Anomaly fixture hosts.
use crate::native_profile::PROFILES;
use std::ffi::{CStr, c_char};
use std::slice;

#[cfg(feature = "sr-fixtures")]
mod contract;

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_test_anomaly_select_build(
    argc: i32,
    argv: *const *const c_char,
) -> usize {
    if argc <= 1 {
        return 0;
    }
    // The fixture supplies its native argv.
    let arguments = unsafe { slice::from_raw_parts(argv, argc as usize) };
    for &argument in &arguments[1..] {
        let argument = unsafe { CStr::from_ptr(argument) };
        if let Some(index) = PROFILES
            .iter()
            .position(|profile| profile.label == argument)
        {
            return index;
        }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_announce_build(
    label: *const c_char,
    executable: *const c_char,
) {
    let label = unsafe { CStr::from_ptr(label) }.to_string_lossy();
    let executable = unsafe { CStr::from_ptr(executable) }.to_string_lossy();
    println!("build profile: {label} ({executable})");
}
