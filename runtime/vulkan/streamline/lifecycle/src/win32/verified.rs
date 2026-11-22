// SPDX-License-Identifier: MIT
//! Admission of the interposer through the same handle the loader holds open.
use std::os::windows::io::{FromRawHandle, IntoRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Cryptography::{
    CALG_SHA_256, CRYPT_VERIFYCONTEXT, CryptAcquireContextW, CryptCreateHash, CryptDestroyHash,
    CryptGetHashParam, CryptHashData, CryptReleaseContext, HP_HASHVAL, PROV_RSA_AES,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_BEGIN, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, OPEN_EXISTING, ReadFile,
    SetFilePointerEx,
};

fn signature_valid(path: *const u16, input: HANDLE) -> bool {
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path,
        hFile: input,
        ..Default::default()
    };
    let mut trust = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..Default::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let result = unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    trust.dwStateAction = WTD_STATEACTION_CLOSE;
    unsafe {
        WinVerifyTrust(
            null_mut(),
            &mut action,
            (&mut trust as *mut WINTRUST_DATA).cast(),
        )
    };
    result == 0
}

#[derive(Default)]
struct Digest {
    provider: usize,
    hash: usize,
}

impl Drop for Digest {
    fn drop(&mut self) {
        unsafe {
            if self.hash != 0 {
                CryptDestroyHash(self.hash);
            }
            if self.provider != 0 {
                CryptReleaseContext(self.provider, 0);
            }
        }
    }
}

fn hash_matches(input: HANDLE, expected: &[u8; 32]) -> bool {
    let mut digest = Digest::default();
    if unsafe {
        CryptAcquireContextW(
            &mut digest.provider,
            null(),
            null(),
            PROV_RSA_AES,
            CRYPT_VERIFYCONTEXT,
        )
    } == 0
        || unsafe { CryptCreateHash(digest.provider, CALG_SHA_256, 0, 0, &mut digest.hash) } == 0
    {
        return false;
    }
    let mut bytes = [0; 64 * 1024];
    loop {
        let mut count = 0;
        if unsafe {
            ReadFile(
                input,
                bytes.as_mut_ptr(),
                bytes.len() as u32,
                &mut count,
                null_mut(),
            )
        } == 0
        {
            return false;
        }
        if count == 0 {
            break;
        }
        if unsafe { CryptHashData(digest.hash, bytes.as_ptr(), count, 0) } == 0 {
            return false;
        }
    }
    let mut actual = [0; 32];
    let mut size = actual.len() as u32;
    (unsafe { CryptGetHashParam(digest.hash, HP_HASHVAL, actual.as_mut_ptr(), &mut size, 0) }) != 0
        && size == 32
        && actual == *expected
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_open_verified(
    path: *const u16,
    expected: &[u8; 32],
    require_signature: u32,
) -> HANDLE {
    let input = unsafe {
        CreateFileW(
            path,
            GENERIC_READ,
            FILE_SHARE_READ,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_SEQUENTIAL_SCAN,
            null_mut(),
        )
    };
    if input == INVALID_HANDLE_VALUE {
        return input;
    }
    // SAFETY: CreateFileW returned an owned, non-inheritable file handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(input) };
    let signed = signature_valid(path, input);
    if (require_signature != 0 && !signed)
        || unsafe { SetFilePointerEx(input, 0, null_mut(), FILE_BEGIN) } == 0
        || !hash_matches(input, expected)
    {
        return INVALID_HANDLE_VALUE;
    }
    // The lifecycle closes only after LoadLibraryExW returns.
    handle.into_raw_handle()
}
