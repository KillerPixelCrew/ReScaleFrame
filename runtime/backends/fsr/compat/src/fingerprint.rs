// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use std::slice;
use windows::{
    Win32::{
        Foundation::HMODULE,
        Storage::FileSystem::{
            FILE_SHARE_READ, GetFileAttributesExW, GetFileExInfoStandard, WIN32_FILE_ATTRIBUTE_DATA,
        },
        System::LibraryLoader::GetModuleFileNameW,
    },
    core::PCWSTR,
};

/// Returns the module file's size and SHA-256 when the size is one of `sizes`.
/// The digest is read only for an admitted size.
pub(super) unsafe fn module_digest(module: *mut c_void, sizes: &[u32]) -> Option<(u32, [u8; 32])> {
    const CAPACITY: usize = 32768;
    let mut path = Vec::new();
    path.try_reserve_exact(CAPACITY).ok()?;
    path.resize(CAPACITY, 0);
    let len = unsafe { GetModuleFileNameW(Some(HMODULE(module)), &mut path) } as usize;
    if len == 0 || len >= path.len() {
        return None;
    }
    let mut attributes = WIN32_FILE_ATTRIBUTE_DATA::default();
    if unsafe {
        GetFileAttributesExW(
            PCWSTR(path.as_ptr()),
            GetFileExInfoStandard,
            (&raw mut attributes).cast(),
        )
    }
    .is_err()
        || attributes.nFileSizeHigh != 0
    {
        return None;
    }
    let file_size = attributes.nFileSizeLow;
    if !sizes.contains(&file_size) {
        return None;
    }
    let digest = unsafe { rsf_runtime_common::hash_file(path.as_ptr(), FILE_SHARE_READ.0) }?;
    Some((file_size, digest))
}

/// The entry at `rva` when its first bytes are `expected`, otherwise null.
pub(super) unsafe fn match_entry(module: *mut c_void, rva: usize, expected: &[u8]) -> *mut c_void {
    // Admission has matched an exact SDK fingerprint before using the recorded RVA.
    let target = unsafe { module.cast::<u8>().add(rva) };
    let bytes = unsafe { slice::from_raw_parts(target, expected.len()) };
    if bytes == expected {
        target.cast()
    } else {
        std::ptr::null_mut()
    }
}
