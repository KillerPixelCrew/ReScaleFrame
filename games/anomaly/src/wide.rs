// SPDX-License-Identifier: MIT
use std::slice;

/// Reads through the first NUL and returns both views of the borrowed string.
///
/// # Safety
/// A non-null pointer must be aligned and readable through its NUL terminator
/// for the returned lifetime.
pub(crate) unsafe fn terminated<'a>(text: *const u16) -> Option<(&'a [u16], &'a [u16])> {
    if text.is_null() {
        return None;
    }
    let mut length = 0;
    while unsafe { text.add(length).read() } != 0 {
        length += 1;
    }
    let terminated = unsafe { slice::from_raw_parts(text, length + 1) };
    Some((&terminated[..length], terminated))
}
