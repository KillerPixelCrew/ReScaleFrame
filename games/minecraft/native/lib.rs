// SPDX-License-Identifier: MIT
//! Minecraft Java 1.21.1 native adapter. Links the shared Runtime and reports GLFW and renderer facts.

#![cfg(windows)]

mod frame;
#[cfg(not(test))]
mod jni;
mod observer;
mod platform_abi;
mod status;

use std::ffi::{CString, OsString, c_void};
use std::os::windows::ffi::OsStringExt;

fn runtime_directory(units: &[u16]) -> Option<(OsString, CString)> {
    let utf8 = CString::new(String::from_utf16(units).ok()?).ok()?;
    Some((OsString::from_wide(units), utf8))
}

#[cfg(test)]
#[test]
fn supplementary_runtime_directory_keeps_both_encodings() {
    use std::os::windows::ffi::OsStrExt;

    let expected = r"C:\RSF\世界🌍\native";
    let utf16: Vec<_> = expected.encode_utf16().collect();
    let (windows, runtime) = runtime_directory(&utf16).unwrap();
    assert!(windows.encode_wide().eq(utf16));
    assert_eq!(runtime.to_bytes(), expected.as_bytes());
    assert!(runtime_directory(&[0]).is_none());
    assert!(runtime_directory(&[0xd800]).is_none());
}

#[unsafe(no_mangle)]
extern "C" fn rsf_minecraft_observer_install(
    parent: *mut c_void,
    child: *mut c_void,
    sink: Option<observer::Sink>,
) -> bool {
    observer::install(parent, child, sink)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_minecraft_observer_close() {
    observer::close();
}
