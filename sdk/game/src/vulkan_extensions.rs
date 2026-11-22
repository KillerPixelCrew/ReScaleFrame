// SPDX-License-Identifier: MIT

use std::ffi::{CStr, c_char};

pub const WIN32_INTEROP: [&CStr; 2] = [
    c"VK_KHR_external_memory_win32",
    c"VK_KHR_external_semaphore_win32",
];

/// Preserve the caller's list and append absent names in requirement order.
/// Null base entries are passed through to Vulkan for validation. A null requirement is refused.
///
/// # Safety
/// Every non-null pointer names a NUL-terminated string valid through the create call.
pub unsafe fn merge(
    base: &[*const c_char],
    required: &[*const c_char],
) -> Option<Vec<*const c_char>> {
    let capacity = base.len().checked_add(required.len())?;
    u32::try_from(capacity).ok()?;
    let mut names = Vec::new();
    names.try_reserve_exact(capacity).ok()?;
    names.extend_from_slice(base);
    for &name in required {
        if name.is_null() {
            return None;
        }
        let required_name = unsafe { CStr::from_ptr(name) };
        if !names.iter().any(|&existing| {
            !existing.is_null() && unsafe { CStr::from_ptr(existing) } == required_name
        }) {
            names.push(name);
        }
    }
    Some(names)
}
