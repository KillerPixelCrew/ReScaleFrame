// SPDX-License-Identifier: MIT
use std::ffi::CStr;

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
