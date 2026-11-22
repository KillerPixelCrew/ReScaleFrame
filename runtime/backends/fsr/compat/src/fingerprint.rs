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

unsafe extern "C" {
    fn rsf_common_sha256_file(path: *const u16, sharing: u32, digest: *mut u8) -> u32;
}

pub(super) unsafe extern "C" fn rsf_fsr4_compat_module_digest(
    module: *mut c_void,
    sizes: *const u32,
    size_count: usize,
    size: *mut u32,
    digest: *mut u8,
) -> bool {
    const CAPACITY: usize = 32768;
    let mut path = Vec::new();
    if path.try_reserve_exact(CAPACITY).is_err() {
        return false;
    }
    path.resize(CAPACITY, 0);
    let len = unsafe { GetModuleFileNameW(Some(HMODULE(module)), &mut path) } as usize;
    if len == 0 || len >= path.len() {
        return false;
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
        return false;
    }
    let file_size = attributes.nFileSizeLow;
    let allowed = unsafe { slice::from_raw_parts(sizes, size_count) };
    if !allowed.contains(&file_size)
        || unsafe { rsf_common_sha256_file(path.as_ptr(), FILE_SHARE_READ.0, digest) } == 0
    {
        return false;
    }
    unsafe { size.write(file_size) };
    true
}

pub(super) unsafe extern "C" fn rsf_fsr4_compat_match_entry(
    module: *mut c_void,
    rva: usize,
    expected: *const u8,
    length: usize,
) -> *mut c_void {
    // Admission has matched an exact SDK fingerprint before using the recorded RVA.
    let target = unsafe { module.cast::<u8>().add(rva) };
    let bytes = unsafe { slice::from_raw_parts(target, length) };
    let expected = unsafe { slice::from_raw_parts(expected, length) };
    if bytes == expected {
        target.cast()
    } else {
        std::ptr::null_mut()
    }
}
