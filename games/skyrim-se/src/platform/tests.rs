// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn wide_names_are_terminated_utf16() {
    assert_eq!(wide("a.dll"), [0x61, 0x2e, 0x64, 0x6c, 0x6c, 0]);
}

#[test]
fn register_refuses_missing_short_and_foreign_tables() {
    assert!(unsafe { register(ptr::null()) }.is_none());
    // Only the header is read from a refused table.
    let short = [8u32, PLATFORM_VERSION, 0, 0];
    assert!(unsafe { register(short.as_ptr().cast()) }.is_none());
    let foreign = [size_of::<Platform>() as u32, PLATFORM_VERSION + 1, 0, 0];
    assert!(unsafe { register(foreign.as_ptr().cast()) }.is_none());
    assert!(get().is_none());
}

#[test]
fn call_record_header_is_checked() {
    let mut call = unsafe { std::mem::zeroed::<CreationCall>() };
    assert!(!call.valid());
    call.struct_size = size_of::<CreationCall>() as u32;
    call.version = PLATFORM_VERSION;
    assert!(call.valid());
}
