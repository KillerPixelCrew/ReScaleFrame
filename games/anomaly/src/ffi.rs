// SPDX-License-Identifier: MIT

/// Borrows a native span whose empty representation may use a null pointer.
///
/// # Safety
/// For nonzero count, the pointer must be nonnull and aligned, and the count
/// must describe initialized elements in one allocation occupying at most
/// isize::MAX bytes. The elements must stay live and immutable for the returned
/// borrow. Callers must tie that borrow to the enclosing profile or callback.
pub(crate) unsafe fn borrow_span<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, count) }
    }
}
