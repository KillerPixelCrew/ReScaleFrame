// SPDX-License-Identifier: MIT
#![cfg(windows)]

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider,
    BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData,
    BCryptOpenAlgorithmProvider,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, OPEN_EXISTING, ReadFile,
};

struct File(HANDLE);

impl Drop for File {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Algorithm(BCRYPT_ALG_HANDLE);

impl Drop for Algorithm {
    fn drop(&mut self) {
        unsafe { BCryptCloseAlgorithmProvider(self.0, 0) };
    }
}

struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        unsafe { BCryptDestroyHash(self.0) };
    }
}

unsafe fn hash_file(path: *const u16, sharing: u32) -> Option<[u8; 32]> {
    let file = unsafe {
        CreateFileW(
            path,
            GENERIC_READ,
            sharing,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if file == INVALID_HANDLE_VALUE {
        return None;
    }
    let file = File(file);

    let mut algorithm = null_mut();
    if unsafe { BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, null(), 0) }
        < 0
    {
        return None;
    }
    let algorithm = Algorithm(algorithm);

    let mut hash = null_mut();
    if unsafe { BCryptCreateHash(algorithm.0, &mut hash, null_mut(), 0, null(), 0, 0) } < 0 {
        return None;
    }
    let hash = Hash(hash);

    let mut buffer = [0; 65536];
    loop {
        let mut read = 0;
        if unsafe {
            ReadFile(
                file.0,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                &mut read,
                null_mut(),
            )
        } == 0
        {
            return None;
        }
        if read == 0 {
            break;
        }
        if unsafe { BCryptHashData(hash.0, buffer.as_ptr(), read, 0) } < 0 {
            return None;
        }
    }

    let mut digest = [0; 32];
    let status = unsafe { BCryptFinishHash(hash.0, digest.as_mut_ptr(), digest.len() as u32, 0) };
    (status >= 0).then_some(digest)
}

// The path is terminated UTF-16. The caller supplies 32 writable bytes.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_common_sha256_file(
    path: *const u16,
    sharing: u32,
    output: *mut [u8; 32],
) -> u32 {
    if path.is_null() || output.is_null() {
        return 0;
    }
    let Some(digest) = (unsafe { hash_file(path, sharing) }) else {
        return 0;
    };
    unsafe { output.write(digest) };
    1
}
