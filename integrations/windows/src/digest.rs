// SPDX-License-Identifier: MIT
use std::fs::OpenOptions;
use std::io::Read;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash,
    BCryptFinishHash, BCryptHashData,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

struct Hash(BCRYPT_HASH_HANDLE);

impl Drop for Hash {
    fn drop(&mut self) {
        unsafe { BCryptDestroyHash(self.0) };
    }
}

/// Streams the file with full sharing so binaries that are in use can be fingerprinted.
pub fn sha256_file(path: &Path) -> Option<[u8; 32]> {
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
        .ok()?;
    let mut handle = ptr::null_mut();
    let status = unsafe {
        BCryptCreateHash(
            BCRYPT_SHA256_ALG_HANDLE,
            &mut handle,
            ptr::null_mut(),
            0,
            ptr::null(),
            0,
            0,
        )
    };
    if status < 0 || handle.is_null() {
        return None;
    }
    let hash = Hash(handle);
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        if unsafe { BCryptHashData(hash.0, buffer.as_ptr(), read as u32, 0) } < 0 {
            return None;
        }
    }
    let mut digest = [0; 32];
    let status = unsafe { BCryptFinishHash(hash.0, digest.as_mut_ptr(), digest.len() as u32, 0) };
    (status >= 0).then_some(digest)
}
