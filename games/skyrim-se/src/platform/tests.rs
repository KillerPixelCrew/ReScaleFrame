// SPDX-License-Identifier: MIT
use super::*;

#[test]
fn call_record_header_is_checked() {
    let mut call = unsafe { std::mem::zeroed::<CreationCall>() };
    assert!(!call.valid());
    call.struct_size = size_of::<CreationCall>() as u32;
    call.version = CREATION_CALL_VERSION;
    assert!(call.valid());
}
