// SPDX-License-Identifier: MIT
//! Process identity and guarded native reads for the Wingman carrier and plugin.
use std::ffi::{OsString, c_void};
use std::io::Read;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_HASH_HANDLE, BCRYPT_SHA256_ALG_HANDLE, BCryptCreateHash, BCryptDestroyHash,
    BCryptFinishHash, BCryptHashData,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

pub(crate) fn module_path(module: HMODULE, buffer: &mut [u16]) -> Option<&[u16]> {
    let length =
        unsafe { GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (length != 0 && length < buffer.len()).then(|| &buffer[..length])
}

pub(crate) fn matches_executable(path: &[u16]) -> bool {
    let leaf = path
        .rsplit(|&unit| matches!(unit, 0x5c | 0x2f))
        .next()
        .unwrap_or_default();
    let expected = crate::EXECUTABLE.to_bytes();
    leaf.len() == expected.len()
        && leaf.iter().zip(expected).all(|(&value, expected)| {
            u8::try_from(value).is_ok_and(|value| value.eq_ignore_ascii_case(expected))
        })
}

pub(crate) fn fingerprint(path: &[u16]) -> Option<[u8; 65]> {
    struct Hash(BCRYPT_HASH_HANDLE);
    impl Drop for Hash {
        fn drop(&mut self) {
            unsafe { BCryptDestroyHash(self.0) };
        }
    }
    let path = PathBuf::from(OsString::from_wide(path));
    let mut file = std::fs::File::open(path).ok()?;
    let mut handle = null_mut();
    let status = unsafe {
        BCryptCreateHash(
            BCRYPT_SHA256_ALG_HANDLE,
            &mut handle,
            null_mut(),
            0,
            null(),
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
        let count = file.read(&mut buffer).ok()?;
        if count == 0 {
            break;
        }
        if unsafe { BCryptHashData(hash.0, buffer.as_ptr(), count as u32, 0) } < 0 {
            return None;
        }
    }
    let mut digest = [0_u8; 32];
    if unsafe { BCryptFinishHash(hash.0, digest.as_mut_ptr(), digest.len() as u32, 0) } < 0 {
        return None;
    }
    let mut hex = [0; 65];
    let alphabet = b"0123456789abcdef";
    for (pair, byte) in hex[..64].as_chunks_mut::<2>().0.iter_mut().zip(digest) {
        pair[0] = alphabet[usize::from(byte >> 4)];
        pair[1] = alphabet[usize::from(byte & 15)];
    }
    Some(hex)
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_wingman_copy(
    output: *mut c_void,
    input: *const c_void,
    bytes: usize,
) -> bool {
    let mut copied = 0;
    !output.is_null()
        && !input.is_null()
        && unsafe { ReadProcessMemory(GetCurrentProcess(), input, output, bytes, &mut copied) } != 0
        && copied == bytes
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_verify_executable() -> bool {
    let mut executable = vec![0; 32768];
    module_path(null_mut(), &mut executable)
        .filter(|path| matches_executable(path))
        .and_then(fingerprint)
        .is_some_and(|digest| &digest[..64] == crate::SHA256.to_bytes())
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_image_base() -> usize {
    unsafe { GetModuleHandleW(null()) as usize }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_wingman_tick_count() -> u64 {
    unsafe { GetTickCount64() }
}
