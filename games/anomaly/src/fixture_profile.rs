// SPDX-License-Identifier: MIT
//! Profile arguments shared by the native Anomaly fixture hosts.
use std::ffi::{CStr, c_char};
use std::slice;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_test_anomaly_select_build(
    argc: i32,
    argv: *const *const c_char,
    labels: *const *const c_char,
    count: usize,
) -> usize {
    if argc <= 1 || count == 0 {
        return 0;
    }
    // The fixture supplies its native argv and the immutable profile labels.
    let arguments = unsafe { slice::from_raw_parts(argv, argc as usize) };
    let labels = unsafe { slice::from_raw_parts(labels, count) };
    for &argument in &arguments[1..] {
        let argument = unsafe { CStr::from_ptr(argument) };
        if let Some(index) = labels
            .iter()
            .position(|&label| unsafe { CStr::from_ptr(label) } == argument)
        {
            return index;
        }
    }
    0
}
