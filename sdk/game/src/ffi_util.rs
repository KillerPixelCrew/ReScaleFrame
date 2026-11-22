// SPDX-License-Identifier: MIT
//! Allocation and string helpers for native ABI callers.
use std::alloc::{Layout, alloc};
use std::ptr::NonNull;

/// Allocates a nonzero-sized value without aborting on allocation failure.
/// Convert the result with `Box::into_raw` when transferring ownership across the ABI.
pub fn try_box<T>(value: T) -> Option<Box<T>> {
    const { assert!(size_of::<T>() != 0) };
    let raw = NonNull::new(unsafe { alloc(Layout::new::<T>()) }.cast::<T>())?;
    unsafe {
        raw.as_ptr().write(value);
        Some(Box::from_raw(raw.as_ptr()))
    }
}

/// Widens exactly N ASCII bytes. Include a NUL in the input when the caller needs one.
pub const fn ascii_wide<const N: usize>(text: &[u8]) -> [u16; N] {
    assert!(text.len() == N);
    let mut wide = [0; N];
    let mut index = 0;
    while index < N {
        assert!(text[index].is_ascii());
        wide[index] = text[index] as u16;
        index += 1;
    }
    wide
}
