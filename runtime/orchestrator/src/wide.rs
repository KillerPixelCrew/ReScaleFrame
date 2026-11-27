// SPDX-License-Identifier: MIT
//! NUL-terminated UTF-16 strings for Win32 calls.

/// Copies a NUL-terminated wide string, terminator included.
///
/// # Safety
/// `text` must point to a readable NUL-terminated UTF-16 string.
pub(crate) unsafe fn copy_nul(text: *const u16) -> Vec<u16> {
    let mut length = 0;
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    unsafe { std::slice::from_raw_parts(text, length + 1) }.to_vec()
}

/// Encodes `text` with a NUL terminator.
pub(crate) fn from_str(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
