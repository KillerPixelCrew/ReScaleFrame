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

unsafe fn open_verified(path: *const u16, expected: &[u8; 32], require_signature: bool) -> HANDLE {
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
    if (require_signature && !signed)
        || unsafe { SetFilePointerEx(input, 0, null_mut(), FILE_BEGIN) } == 0
        || !hash_matches(input, expected)
    {
        return INVALID_HANDLE_VALUE;
    }
    // The lifecycle closes only after LoadLibraryExW returns.
    handle.into_raw_handle()
}

/// SHA-256 of the pinned retail sl.interposer.dll, admitted with its Authenticode signature.
const RETAIL: [u8; 32] = [
    0x8c, 0x87, 0xc9, 0x49, 0x94, 0x61, 0xda, 0x56, 0x1e, 0xdd, 0x52, 0x9a, 0xa9, 0xbf, 0x78, 0x31,
    0xd6, 0x7d, 0x7b, 0x94, 0xeb, 0xb1, 0xc1, 0xa5, 0xed, 0x54, 0xef, 0x49, 0x34, 0xe1, 0xea, 0x4c,
];

/// The corrected build's interposer digest from its verified receipt
/// (runtime/vulkan/streamline/verify-receipt.cmake). That build is unsigned.
const CORRECTED: Option<[u8; 32]> = match option_env!("RSF_CORRECTED_STREAMLINE_SHA256") {
    Some(hex) => Some(digest(hex)),
    None => None,
};

/// Parses 64 hex digits at compile time; anything else fails the build.
const fn digest(hex: &str) -> [u8; 32] {
    const fn nibble(digit: u8) -> u8 {
        match digit {
            b'0'..=b'9' => digit - b'0',
            b'a'..=b'f' => digit - b'a' + 10,
            b'A'..=b'F' => digit - b'A' + 10,
            _ => panic!("RSF_CORRECTED_STREAMLINE_SHA256 must be hexadecimal"),
        }
    }
    let hex = hex.as_bytes();
    assert!(hex.len() == 64, "RSF_CORRECTED_STREAMLINE_SHA256 must be a SHA-256");
    let mut out = [0; 32];
    let mut index = 0;
    while index < 32 {
        out[index] = nibble(hex[2 * index]) << 4 | nibble(hex[2 * index + 1]);
        index += 1;
    }
    out
}

/// Opens the interposer the build admits: the corrected build by digest, otherwise the pinned
/// retail build by digest and signature. Returns the verified handle or INVALID_HANDLE_VALUE.
pub(super) unsafe fn open_interposer(path: *const u16) -> HANDLE {
    match CORRECTED {
        Some(expected) => unsafe { open_verified(path, &expected, false) },
        None => unsafe { open_verified(path, &RETAIL, true) },
    }
}