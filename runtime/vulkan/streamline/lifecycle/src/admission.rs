// SPDX-License-Identifier: MIT
//! Admission of `sl.interposer.dll`, for every Streamline owner. A module is admitted by one of
//! two requirements:
//!
//! - NVIDIA's key ([`nvidia_signed`], the D3D owners): NVIDIA signs the interposer twice, the
//!   OS-trusted primary signature and a nested secondary signature whose signer's RSA public key is
//!   NVIDIA's Streamline key. The file must pass the OS strong-signing policy, carry exactly one
//!   secondary signature, and that signature's certificate must hold the pinned key. This is
//!   written from that behaviour, not from the vendor's header.
//! - A pinned digest ([`open_by_digest`], the Vulkan lifecycle): the SHA-256 of the exact build,
//!   read through the handle the loader then holds open. The corrected interposer is unsigned, so
//!   only the retail build also needs its Authenticode signature.
//!
//! `wintrust.dll` and `crypt32.dll` are loaded from System32 on first use, never by a load-time
//! import, so the game directory cannot supply them.
use crate::sdk_api::export;
use std::ffi::{CStr, c_char, c_void};
use std::os::windows::io::{FromRawHandle, IntoRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Cryptography::{
    CALG_SHA_256, CERT_CONTEXT, CERT_FIND_SUBJECT_CERT, CERT_INFO,
    CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
    CERT_QUERY_OBJECT_FILE, CERT_STORE_PROV_PKCS7, CERT_STRONG_SIGN_OID_INFO_CHOICE,
    CERT_STRONG_SIGN_PARA, CERT_STRONG_SIGN_PARA_0, CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM,
    CNG_RSA_PUBLIC_KEY_BLOB, CRYPT_ENCODE_ALLOC_FLAG, CRYPT_INTEGER_BLOB, CRYPT_VERIFYCONTEXT,
    CryptAcquireContextW, CryptCreateHash, CryptDestroyHash, CryptGetHashParam, CryptHashData,
    CryptReleaseContext, HP_HASHVAL, PKCS_7_ASN_ENCODING, PROV_RSA_AES, X509_ASN_ENCODING,
};
use windows_sys::Win32::Security::WinTrust::{
    WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
    WINTRUST_SIGNATURE_SETTINGS, WSS_GET_SECONDARY_SIG_COUNT, WSS_VERIFY_SPECIFIC,
    WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
    WTD_STATEACTION_VERIFY, WTD_UI_NONE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_BEGIN, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, OPEN_EXISTING, ReadFile,
    SetFilePointerEx,
};
use windows_sys::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows_sys::core::{GUID, w};

type Handle = *mut c_void;
type Bool = i32;

/// Object identifier of the nested (secondary) signature attribute in a PE signature.
const NESTED_SIGNATURE: &CStr = c"1.3.6.1.4.1.311.2.4.1";
/// `szOID_CERT_STRONG_SIGN_OS_CURRENT`, the OS's current strong-signing policy.
const STRONG_SIGN_POLICY: &CStr = c"1.3.6.1.4.1.311.72.1.1";
const ENCODING: u32 = X509_ASN_ENCODING | PKCS_7_ASN_ENCODING;

/// The CNG RSA public key blob (`BCRYPT_RSAKEY_BLOB` header, exponent, modulus) of the key that
/// signs the secondary signature of NVIDIA's Streamline modules, as published in the Streamline SDK
/// security header. The unit test below checks it against the signature of the shipped 2.14.1
/// `sl.interposer.dll`.
const NVIDIA_STREAMLINE_KEY: [u8; 411] = [
    0x52, 0x53, 0x41, 0x31, 0x00, 0x0c, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0xc1, 0x8e, 0x40, 0xc3, 0xf5,
    0xa7, 0x01, 0x9a, 0x37, 0x6b, 0x47, 0xa8, 0x58, 0xe8, 0xbe, 0xe3, 0x55, 0x0a, 0xee, 0x0f, 0x0d,
    0x32, 0xaa, 0x12, 0xf9, 0x56, 0x7f, 0x5d, 0xfd, 0x82, 0x09, 0x33, 0x21, 0x42, 0xf2, 0xe8, 0x74,
    0x98, 0x51, 0xb3, 0x88, 0x74, 0xcd, 0x00, 0x6e, 0xb1, 0x08, 0x10, 0x4b, 0xf1, 0xda, 0xd6, 0x97,
    0x87, 0xd4, 0x9c, 0xb1, 0x13, 0xa8, 0xa2, 0x86, 0x15, 0x0e, 0xc1, 0xa5, 0x9c, 0xe5, 0x90, 0x9b,
    0xbe, 0x69, 0xdc, 0x6a, 0x82, 0xbe, 0xb4, 0x4b, 0x4b, 0xfa, 0x95, 0x8e, 0xc1, 0xfc, 0x2b, 0x61,
    0x95, 0xd1, 0x91, 0xed, 0xeb, 0x87, 0xe7, 0x09, 0x84, 0x05, 0x41, 0x03, 0xb0, 0x2d, 0xd4, 0x39,
    0x7f, 0x62, 0x06, 0x56, 0x33, 0x93, 0x7e, 0x77, 0x54, 0x06, 0x77, 0x2b, 0x75, 0x05, 0xbc, 0xeb,
    0x98, 0xea, 0xc0, 0xa2, 0xca, 0x98, 0x86, 0x0f, 0x10, 0x65, 0xde, 0x19, 0x2c, 0xa6, 0x1e, 0x93,
    0xb0, 0x92, 0x5d, 0x5f, 0x5b, 0x6f, 0x79, 0x6d, 0x2c, 0x76, 0xa6, 0x67, 0x50, 0xaa, 0x8f, 0xc2,
    0x4c, 0xf1, 0x08, 0xf7, 0xc0, 0x27, 0x29, 0xf0, 0x68, 0xf4, 0x64, 0x00, 0x1c, 0xb6, 0x28, 0x1e,
    0x25, 0xb8, 0xf3, 0x8a, 0xd1, 0x6e, 0x65, 0xa3, 0x61, 0x9d, 0xf8, 0xca, 0x4a, 0x41, 0x60, 0x80,
    0x62, 0xdf, 0x41, 0xa4, 0x8b, 0xdc, 0x97, 0xee, 0xeb, 0x64, 0x6f, 0xe4, 0x8f, 0x4b, 0xdf, 0x24,
    0x01, 0x80, 0xd9, 0xb4, 0x0a, 0xec, 0x0d, 0x3e, 0xb7, 0x76, 0xba, 0xe9, 0xe7, 0xde, 0x07, 0xdd,
    0x30, 0xc8, 0x4a, 0x14, 0x79, 0xec, 0x15, 0xed, 0x5c, 0xc6, 0xcc, 0xd4, 0xe6, 0x06, 0x3c, 0x42,
    0x92, 0x10, 0xf7, 0x7c, 0x80, 0x1e, 0x78, 0xd3, 0xb4, 0x9f, 0xc2, 0x3b, 0xa8, 0x7b, 0xa0, 0xe3,
    0x0c, 0xd9, 0xad, 0x2e, 0x09, 0x72, 0xe2, 0x8f, 0x54, 0x28, 0x87, 0x3c, 0xba, 0x7c, 0x97, 0x80,
    0xdc, 0x09, 0xb5, 0x12, 0x34, 0x78, 0x9a, 0x26, 0xd0, 0xa3, 0xa7, 0xa7, 0x1b, 0x25, 0x19, 0xe5,
    0x6e, 0xbe, 0xd7, 0x5a, 0x91, 0x32, 0xc4, 0xa9, 0x2f, 0xcc, 0xd5, 0x82, 0x4b, 0x5b, 0x9f, 0xad,
    0xf3, 0x2f, 0xed, 0x4f, 0x33, 0xe1, 0x50, 0x33, 0xd6, 0x90, 0x79, 0x22, 0xe5, 0x1c, 0xc7, 0x35,
    0xe7, 0x58, 0xe6, 0xb4, 0x8b, 0xc4, 0x28, 0x20, 0xec, 0xca, 0x70, 0xbb, 0x02, 0x1b, 0x48, 0xd8,
    0x84, 0x51, 0x24, 0x33, 0x2a, 0x08, 0xb1, 0x15, 0x4e, 0xbc, 0x88, 0xa5, 0xe1, 0x37, 0x76, 0x70,
    0xe6, 0xdf, 0x3f, 0x73, 0xfd, 0x0d, 0x8a, 0xd9, 0x0d, 0xa5, 0x35, 0xb2, 0xb4, 0x01, 0x42, 0x96,
    0xc4, 0xaa, 0x1c, 0xeb, 0x68, 0x62, 0x36, 0xbf, 0xef, 0x5e, 0x2a, 0x3d, 0x18, 0x91, 0x8b, 0x92,
    0x0a, 0x1e, 0xce, 0x98, 0x5b, 0x7b, 0x64, 0x42, 0x09, 0xb0, 0x1d,
];

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
    assert!(
        hex.len() == 64,
        "RSF_CORRECTED_STREAMLINE_SHA256 must be a SHA-256"
    );
    let mut out = [0; 32];
    let mut index = 0;
    while index < 32 {
        out[index] = nibble(hex[2 * index]) << 4 | nibble(hex[2 * index + 1]);
        index += 1;
    }
    out
}

type WinVerifyTrust = unsafe extern "system" fn(HANDLE, *mut GUID, *mut c_void) -> i32;
type CryptQueryObject = unsafe extern "system" fn(
    u32,
    *const c_void,
    u32,
    u32,
    u32,
    *mut u32,
    *mut u32,
    *mut u32,
    *mut Handle,
    *mut Handle,
    *mut *const c_void,
) -> Bool;
type CryptMsgGetParam = unsafe extern "system" fn(Handle, u32, u32, *mut c_void, *mut u32) -> Bool;
type CryptMsgOpenToDecode =
    unsafe extern "system" fn(u32, u32, u32, usize, *mut c_void, *mut c_void) -> Handle;
type CryptMsgUpdate = unsafe extern "system" fn(Handle, *const u8, u32, Bool) -> Bool;
type CryptMsgClose = unsafe extern "system" fn(Handle) -> Bool;
type CertOpenStore =
    unsafe extern "system" fn(*const c_char, u32, usize, u32, *const c_void) -> Handle;
type CertCloseStore = unsafe extern "system" fn(Handle, u32) -> Bool;
type CertFindCertificateInStore = unsafe extern "system" fn(
    Handle,
    u32,
    u32,
    u32,
    *const c_void,
    *const CERT_CONTEXT,
) -> *const CERT_CONTEXT;
type CertFreeCertificateContext = unsafe extern "system" fn(*const CERT_CONTEXT) -> Bool;
type CryptDecodeObjectEx = unsafe extern "system" fn(
    u32,
    *const c_char,
    *const u8,
    u32,
    u32,
    *const c_void,
    *mut c_void,
    *mut u32,
) -> Bool;

struct Crypt32 {
    query_object: CryptQueryObject,
    get_param: CryptMsgGetParam,
    open_to_decode: CryptMsgOpenToDecode,
    update: CryptMsgUpdate,
    close_message: CryptMsgClose,
    open_store: CertOpenStore,
    close_store: CertCloseStore,
    find_certificate: CertFindCertificateInStore,
    free_certificate: CertFreeCertificateContext,
    decode: CryptDecodeObjectEx,
}

fn crypt32() -> Option<&'static Crypt32> {
    static CRYPT32: OnceLock<Option<Crypt32>> = OnceLock::new();
    CRYPT32
        .get_or_init(|| {
            let module = unsafe {
                LoadLibraryExW(w!("crypt32.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
            };
            if module.is_null() {
                return None;
            }
            unsafe {
                Some(Crypt32 {
                    query_object: export(module, c"CryptQueryObject")?,
                    get_param: export(module, c"CryptMsgGetParam")?,
                    open_to_decode: export(module, c"CryptMsgOpenToDecode")?,
                    update: export(module, c"CryptMsgUpdate")?,
                    close_message: export(module, c"CryptMsgClose")?,
                    open_store: export(module, c"CertOpenStore")?,
                    close_store: export(module, c"CertCloseStore")?,
                    find_certificate: export(module, c"CertFindCertificateInStore")?,
                    free_certificate: export(module, c"CertFreeCertificateContext")?,
                    decode: export(module, c"CryptDecodeObjectEx")?,
                })
            }
        })
        .as_ref()
}

fn wintrust() -> Option<WinVerifyTrust> {
    static WINTRUST: OnceLock<Option<WinVerifyTrust>> = OnceLock::new();
    *WINTRUST.get_or_init(|| {
        let module =
            unsafe { LoadLibraryExW(w!("wintrust.dll"), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
        if module.is_null() {
            return None;
        }
        unsafe { export(module, c"WinVerifyTrust") }
    })
}

/// Runs WinVerifyTrust on the file's primary signature, hands `verdict` whether the OS trusts it
/// while the verification state is open, then closes that state whatever the verdict. `file` may
/// be null; `settings` may ask for the secondary signatures to be counted. False without
/// `wintrust.dll`.
unsafe fn verify_trust(
    path: *const u16,
    file: HANDLE,
    provider_flags: u32,
    settings: *mut WINTRUST_SIGNATURE_SETTINGS,
    verdict: impl FnOnce(bool) -> bool,
) -> bool {
    let Some(verify) = wintrust() else {
        return false;
    };
    let mut file = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: path,
        hFile: file,
        pgKnownSubject: null_mut(),
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: null_mut(),
        pSIPClientData: null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 { pFile: &mut file },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: null_mut(),
        pwszURLReference: null_mut(),
        dwProvFlags: provider_flags,
        dwUIContext: 0,
        pSignatureSettings: settings,
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let data_pointer = &raw mut data;
    let trusted = unsafe { verify(null_mut(), &mut action, data_pointer.cast()) } == 0;
    let admitted = verdict(trusted);
    unsafe {
        (*data_pointer).dwStateAction = WTD_STATEACTION_CLOSE;
        verify(null_mut(), &mut action, data_pointer.cast());
    }
    admitted
}

// Handles released on every path.
struct Message<'a>(&'a Crypt32, Handle);
impl Drop for Message<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.close_message)(self.1) };
    }
}

struct Store<'a>(&'a Crypt32, Handle);
impl Drop for Store<'_> {
    fn drop(&mut self) {
        // CERT_CLOSE_STORE_FORCE_FLAG
        unsafe { (self.0.close_store)(self.1, 1) };
    }
}

struct Certificate<'a>(&'a Crypt32, *const CERT_CONTEXT);
impl Drop for Certificate<'_> {
    fn drop(&mut self) {
        unsafe { (self.0.free_certificate)(self.1) };
    }
}

struct Allocation(*mut c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

/// Signer information of a message, in 8-byte aligned storage.
fn signer_info(crypt: &Crypt32, message: Handle) -> Option<Vec<u64>> {
    let mut size = 0u32;
    if unsafe { (crypt.get_param)(message, CMSG_SIGNER_INFO_PARAM, 0, null_mut(), &mut size) } == 0
        || size == 0
    {
        return None;
    }
    let mut storage = vec![0u64; (size as usize).div_ceil(8)];
    (unsafe {
        (crypt.get_param)(
            message,
            CMSG_SIGNER_INFO_PARAM,
            0,
            storage.as_mut_ptr().cast(),
            &mut size,
        )
    } != 0)
        .then_some(storage)
}

/// Whether the secondary signature's certificate holds NVIDIA's Streamline key.
unsafe fn nested_signer_is_nvidia(crypt: &Crypt32, path: *const u16) -> bool {
    let (mut encoding, mut content, mut format) = (0u32, 0u32, 0u32);
    let (mut store, mut message) = (null_mut(), null_mut());
    if unsafe {
        (crypt.query_object)(
            CERT_QUERY_OBJECT_FILE,
            path.cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &mut encoding,
            &mut content,
            &mut format,
            &mut store,
            &mut message,
            null_mut(),
        )
    } == 0
    {
        return false;
    }
    let _store = Store(crypt, store);
    let _message = Message(crypt, message);
    let Some(primary) = signer_info(crypt, message) else {
        return false;
    };
    let primary = unsafe { &*primary.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    // Only the first nested-signature attribute counts.
    let attributes = primary.UnauthAttrs;
    for index in 0..attributes.cAttr as usize {
        let attribute = unsafe { &*attributes.rgAttr.add(index) };
        if unsafe { CStr::from_ptr(attribute.pszObjId.cast()) } != NESTED_SIGNATURE {
            continue;
        }
        if attribute.cValue == 0 {
            return false;
        }
        return unsafe { nested_key_matches(crypt, &*attribute.rgValue) };
    }
    false
}

unsafe fn nested_key_matches(crypt: &Crypt32, blob: &CRYPT_INTEGER_BLOB) -> bool {
    let message = unsafe { (crypt.open_to_decode)(ENCODING, 0, 0, 0, null_mut(), null_mut()) };
    if message.is_null() {
        return false;
    }
    let _message = Message(crypt, message);
    if unsafe { (crypt.update)(message, blob.pbData, blob.cbData, 1) } == 0 {
        return false;
    }
    let Some(signer) = signer_info(crypt, message) else {
        return false;
    };
    let signer = unsafe { &*signer.as_ptr().cast::<CMSG_SIGNER_INFO>() };
    let store = unsafe {
        (crypt.open_store)(
            CERT_STORE_PROV_PKCS7.cast(),
            ENCODING,
            0,
            0,
            std::ptr::from_ref(blob).cast(),
        )
    };
    if store.is_null() {
        return false;
    }
    let _store = Store(crypt, store);
    // Find the signer's certificate by issuer and serial number.
    let mut wanted: CERT_INFO = unsafe { std::mem::zeroed() };
    wanted.Issuer = signer.Issuer;
    wanted.SerialNumber = signer.SerialNumber;
    let certificate = unsafe {
        (crypt.find_certificate)(
            store,
            ENCODING,
            0,
            CERT_FIND_SUBJECT_CERT,
            std::ptr::from_ref(&wanted).cast(),
            null(),
        )
    };
    if certificate.is_null() {
        return false;
    }
    let certificate = Certificate(crypt, certificate);
    let key = unsafe { &(*(*certificate.1).pCertInfo).SubjectPublicKeyInfo.PublicKey };
    let mut decoded: *mut c_void = null_mut();
    let mut length = 0u32;
    if unsafe {
        (crypt.decode)(
            ENCODING,
            CNG_RSA_PUBLIC_KEY_BLOB.cast(),
            key.pbData,
            key.cbData,
            CRYPT_ENCODE_ALLOC_FLAG,
            null(),
            std::ptr::from_mut(&mut decoded).cast(),
            &mut length,
        )
    } == 0
    {
        return false;
    }
    let decoded = Allocation(decoded);
    !decoded.0.is_null()
        && unsafe { std::slice::from_raw_parts(decoded.0.cast::<u8>(), length as usize) }
            == NVIDIA_STREAMLINE_KEY
}

/// Whether the file at the absolute `path` is an NVIDIA-signed Streamline module.
///
/// # Safety
/// `path` is null or a NUL-terminated UTF-16 path.
pub unsafe fn nvidia_signed(path: *const u16) -> bool {
    if path.is_null() {
        return false;
    }
    let Some(crypt) = crypt32() else {
        return false;
    };
    // Verify the primary signature (index 0) and have the secondary signatures counted.
    let mut policy = CERT_STRONG_SIGN_PARA {
        cbSize: size_of::<CERT_STRONG_SIGN_PARA>() as u32,
        dwInfoChoice: CERT_STRONG_SIGN_OID_INFO_CHOICE,
        Anonymous: CERT_STRONG_SIGN_PARA_0 {
            pszOID: STRONG_SIGN_POLICY.as_ptr().cast_mut().cast(),
        },
    };
    let mut settings = WINTRUST_SIGNATURE_SETTINGS {
        cbStruct: size_of::<WINTRUST_SIGNATURE_SETTINGS>() as u32,
        dwIndex: 0,
        dwFlags: WSS_GET_SECONDARY_SIG_COUNT | WSS_VERIFY_SPECIFIC,
        cSecondarySigs: 0,
        dwVerifiedSigIndex: 0,
        pCryptoPolicy: &mut policy,
    };
    let settings = &raw mut settings;
    unsafe {
        verify_trust(path, null_mut(), 0, settings, |trusted| {
            // The OS verified the primary signature. One secondary signature must exist and be
            // NVIDIA's.
            trusted && (*settings).cSecondarySigs == 1 && nested_signer_is_nvidia(crypt, path)
        })
    }
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
    let signed = unsafe {
        verify_trust(
            path,
            input,
            WTD_CACHE_ONLY_URL_RETRIEVAL,
            null_mut(),
            |trusted| trusted,
        )
    };
    if (require_signature && !signed)
        || unsafe { SetFilePointerEx(input, 0, null_mut(), FILE_BEGIN) } == 0
        || !hash_matches(input, expected)
    {
        return INVALID_HANDLE_VALUE;
    }
    // The lifecycle closes only after LoadLibraryExW returns.
    handle.into_raw_handle()
}

/// Opens the interposer the build admits: the corrected build by digest, otherwise the pinned
/// retail build by digest and signature. Returns the verified handle or INVALID_HANDLE_VALUE.
/// Unit tests reach the lifecycle through a fake leaf instead.
#[cfg_attr(test, allow(dead_code))]
pub(crate) unsafe fn open_by_digest(path: *const u16) -> HANDLE {
    match CORRECTED {
        Some(expected) => unsafe { open_verified(path, &expected, false) },
        None => unsafe { open_verified(path, &RETAIL, true) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsf_game_sys::ffi_util::wide;

    fn interposer() -> Option<std::path::PathBuf> {
        let directory = std::env::var_os("RSF_STREAMLINE_BIN")?;
        let path = std::path::Path::new(&directory).join("sl.interposer.dll");
        path.is_file().then_some(path)
    }

    #[test]
    fn refuses_missing_null_and_unsigned_files() {
        assert!(!unsafe { nvidia_signed(null()) });
        assert!(!unsafe { nvidia_signed(wide(r"C:\definitely\not\here.dll").as_ptr()) });
        let exe = std::env::current_exe().expect("test executable");
        assert!(!unsafe { nvidia_signed(wide(exe.to_str().unwrap()).as_ptr()) });
    }

    #[test]
    fn accepts_the_signed_interposer_and_refuses_a_modified_copy() {
        let Some(path) = interposer() else {
            eprintln!("RSF_STREAMLINE_BIN does not name an SDK bin directory, skipping");
            return;
        };
        let path_text = path.to_str().expect("UTF-8 path");
        assert!(unsafe { nvidia_signed(wide(path_text).as_ptr()) });
        // A flipped byte in the first section breaks the primary signature digest.
        let mut bytes = std::fs::read(&path).expect("interposer bytes");
        let target = bytes.len() / 2;
        bytes[target] ^= 0xff;
        let copy = std::env::temp_dir().join(format!("rsf-sig-test-{}.dll", std::process::id()));
        std::fs::write(&copy, &bytes).expect("write copy");
        let accepted = unsafe { nvidia_signed(wide(copy.to_str().unwrap()).as_ptr()) };
        let _ = std::fs::remove_file(&copy);
        assert!(!accepted);
    }

    #[test]
    fn pinned_key_is_a_cng_rsa_public_blob() {
        // 'RSA1', then bit length 3072 and a 3-byte exponent 0x010001 ahead of a 384-byte modulus.
        assert_eq!(&NVIDIA_STREAMLINE_KEY[..4], b"RSA1");
        let bit_length = u32::from_le_bytes(NVIDIA_STREAMLINE_KEY[4..8].try_into().unwrap());
        let exponent_length = u32::from_le_bytes(NVIDIA_STREAMLINE_KEY[8..12].try_into().unwrap());
        let modulus_length = u32::from_le_bytes(NVIDIA_STREAMLINE_KEY[12..16].try_into().unwrap());
        assert_eq!(
            NVIDIA_STREAMLINE_KEY.len(),
            24 + exponent_length as usize + modulus_length as usize
        );
        assert_eq!(bit_length as usize, modulus_length as usize * 8);
    }
}
