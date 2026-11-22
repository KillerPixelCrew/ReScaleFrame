// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn wide_names_are_terminated_utf16() {
    assert_eq!(wide("a.dll"), [0x61, 0x2e, 0x64, 0x6c, 0x6c, 0]);
}

#[test]
fn call_record_header_is_checked() {
    let mut call = unsafe { std::mem::zeroed::<CreationCall>() };
    assert!(!call.valid());
    call.struct_size = size_of::<CreationCall>() as u32;
    call.version = CREATION_CALL_VERSION;
    assert!(call.valid());
}
