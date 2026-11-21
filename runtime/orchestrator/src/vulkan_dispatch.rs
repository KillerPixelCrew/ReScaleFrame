// SPDX-License-Identifier: MIT
use std::ffi::{CStr, c_char};
use std::ptr;

/// FidelityFX can ask for KHR spellings when the host exposes the promoted core names.
pub(super) fn promoted_device_proc_alias(name: &CStr) -> Option<&'static CStr> {
    match name.to_bytes() {
        b"vkGetBufferMemoryRequirements2KHR" => Some(c"vkGetBufferMemoryRequirements2"),
        b"vkGetImageMemoryRequirements2KHR" => Some(c"vkGetImageMemoryRequirements2"),
        b"vkBindBufferMemory2KHR" => Some(c"vkBindBufferMemory2"),
        b"vkBindImageMemory2KHR" => Some(c"vkBindImageMemory2"),
        _ => None,
    }
}

/// The native leaf supplies a null or NUL-terminated name. A returned alias has static lifetime.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_vk_promoted_device_proc_alias(name: *const c_char) -> *const c_char {
    if name.is_null() {
        return ptr::null();
    }
    promoted_device_proc_alias(unsafe { CStr::from_ptr(name) }).map_or(ptr::null(), CStr::as_ptr)
}
