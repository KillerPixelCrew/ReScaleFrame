// SPDX-License-Identifier: MIT
//! Allocation and string helpers for native ABI callers.
use std::alloc::{Layout, alloc};
use std::ptr::NonNull;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Locks `mutex`, taking the guard of a poisoned one: a panic on a foreign thread must not
/// wedge an ABI entry that has to keep answering.
pub fn lock_or_recover<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

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

/// `text` as NUL-terminated UTF-16.
pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

/// `units` followed by `suffix`, NUL-terminated: a path built from a module directory.
pub fn wide_path(units: &[u16], suffix: &str) -> Vec<u16> {
    units
        .iter()
        .copied()
        .chain(suffix.encode_utf16())
        .chain(Some(0))
        .collect()
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

/// A sized ABI record: non-null and its leading `struct_size` covers `T`.
///
/// # Safety
/// A non-null `value` must point to a readable record whose first field is its `u32` size.
pub unsafe fn has_capacity<T>(value: *const T) -> bool {
    !value.is_null() && (unsafe { value.cast::<u32>().read_unaligned() } as usize) >= size_of::<T>()
}

/// The record behind `value` when [`has_capacity`] admits it.
///
/// # Safety
/// As for [`has_capacity`]; the record must also be suitably aligned for `T` and live for `'a`.
pub unsafe fn checked<'a, T>(value: *const T) -> Option<&'a T> {
    unsafe { has_capacity(value) }.then(|| unsafe { &*value })
}

#[cfg(test)]
mod tests {
    #[test]
    fn wide_strings_are_terminated_utf16() {
        assert_eq!(super::wide("a.dll"), [0x61, 0x2e, 0x64, 0x6c, 0x6c, 0]);
        assert_eq!(super::wide_path(&[0x43], "\\b"), [0x43, 0x5c, 0x62, 0]);
    }
}
