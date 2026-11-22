// SPDX-License-Identifier: MIT
//! The hash-pinned AC7 contact-shadow rewrite. The caller owns the output allocation.
//! The shader facts and operands come from the existing capture evidence.
use crate::shader_hash::{crc32c, dxbc_checksum, dxbc_digest_bytes};
use std::ffi::c_void;
use std::slice;

const ORIGINAL_SIZE: usize = 10_456;
const MAX_INPUT: usize = 8 * 1024 * 1024;
const SHADER_OFFSET: usize = 208;
const SHADER_SIZE: usize = 10_240;
const TOKENS: usize = SHADER_OFFSET + 8;
const PHASE_SPLIT: usize = TOKENS + 469 * 4;
const START_SPLIT: usize = TOKENS + 996 * 4;
const TEMPS: usize = TOKENS + 66 * 4;

const UNMATCHED: i32 = 0;
const PATCHED: i32 = 1;
const REFUSED: i32 = 2;

const EXPECTED: [u8; 32] = [
    0x60, 0xa2, 0xca, 0x61, 0xe5, 0xd8, 0x3b, 0x85, 0x3f, 0x69, 0x2e, 0xe8, 0xc0, 0xbc, 0x35, 0x80,
    0xea, 0x46, 0xb8, 0xe3, 0x3d, 0x72, 0xc6, 0x44, 0x94, 0x9e, 0x5f, 0x37, 0xc1, 0x06, 0x0a, 0x12,
];

const NOISE: [u32; 10] = [
    0x0a00000f, 0x00100012, 1, 0x00101046, 2, 0x00004002, 0x3d897143, 0x3bbf4590, 0, 0,
];

const RAY_START: [u32; 5] = [0x05000036, 0x00100042, 0x0000000a, 0x0010002a, 0x00000008];

const PHASE: [u32; 18] = [
    0x06000056, 0x00100012, 14, 0x0020802a, 0, 137, 0x0c000032, 0x00100032, 14, 0x00100006, 14,
    0x00004002, 0x4202a8f6, 0x413d0a3d, 0, 0, 0x00101046, 2,
];

const BIAS: [u32; 169] = [
    0x08000000, 0x001000f2, 0x0000000f, 0x00101446, 0x00000000, 0x00208ee6, 0x00000000, 0x00000080,
    0x09000000, 0x001000f2, 0x00000010, 0x00101446, 0x00000000, 0x80208ee6, 0x00000041, 0x00000000,
    0x00000080, 0x05000036, 0x00100022, 0x0000000f, 0x0010101a, 0x00000000, 0x05000036, 0x00100022,
    0x00000010, 0x0010101a, 0x00000000, 0x05000036, 0x00100042, 0x0000000f, 0x0010100a, 0x00000000,
    0x05000036, 0x00100042, 0x00000010, 0x0010100a, 0x00000000, 0x8d000048, 0x800000c2, 0x00155543,
    0x00100012, 0x00000011, 0x00100046, 0x0000000f, 0x00107e46, 0x00000004, 0x00106000, 0x00000004,
    0x00004001, 0x00000000, 0x8d000048, 0x800000c2, 0x00155543, 0x00100022, 0x00000011, 0x00100046,
    0x00000010, 0x00107e16, 0x00000004, 0x00106000, 0x00000004, 0x00004001, 0x00000000, 0x8d000048,
    0x800000c2, 0x00155543, 0x00100042, 0x00000011, 0x00100ae6, 0x0000000f, 0x00107c96, 0x00000004,
    0x00106000, 0x00000004, 0x00004001, 0x00000000, 0x8d000048, 0x800000c2, 0x00155543, 0x00100082,
    0x00000011, 0x00100ae6, 0x00000010, 0x00107396, 0x00000004, 0x00106000, 0x00000004, 0x00004001,
    0x00000000, 0x8d000048, 0x800000c2, 0x00155543, 0x00100082, 0x00000010, 0x00101046, 0x00000000,
    0x00107396, 0x00000004, 0x00106000, 0x00000004, 0x00004001, 0x00000000, 0x08000000, 0x001000f2,
    0x00000011, 0x00100e46, 0x00000011, 0x80100ff6, 0x00000041, 0x00000010, 0x09000033, 0x00100012,
    0x0000000f, 0x8010000a, 0x00000081, 0x00000011, 0x8010001a, 0x00000081, 0x00000011, 0x09000033,
    0x00100022, 0x0000000f, 0x8010002a, 0x00000081, 0x00000011, 0x8010003a, 0x00000081, 0x00000011,
    0x09000034, 0x00100012, 0x00000010, 0x8010000a, 0x00000081, 0x00000011, 0x8010001a, 0x00000081,
    0x00000011, 0x09000034, 0x00100022, 0x00000010, 0x8010002a, 0x00000081, 0x00000011, 0x8010003a,
    0x00000081, 0x00000011, 0x09000037, 0x00100032, 0x0000000f, 0x00100046, 0x0000000f, 0x00100046,
    0x0000000f, 0x00100046, 0x00000010, 0x07000000, 0x00100012, 0x0000000f, 0x0010000a, 0x0000000f,
    0x0010001a, 0x0000000f, 0x07000038, 0x00100012, 0x0000000f, 0x0010000a, 0x0000000f, 0x00004001,
    0x3f000000,
];

const BIASED_START: [u32; 7] = [
    0x07000000, 0x00100042, 0x0000000a, 0x0010002a, 0x00000008, 0x0010000a, 0x0000000f,
];

const GROWTH: usize = (PHASE.len() + BIAS.len() + BIASED_START.len() - RAY_START.len()) * 4;
type Resize = unsafe extern "C" fn(*mut c_void, usize) -> *mut u8;

#[cfg(windows)]
fn sha256(input: &[u8]) -> Option<[u8; 32]> {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
    };
    use windows::core::{BOOL, PCWSTR, s, w};

    type Hash = unsafe extern "system" fn(
        PCWSTR,
        u32,
        *const c_void,
        *const u8,
        u32,
        *mut u8,
        *mut u32,
    ) -> BOOL;

    let module =
        unsafe { LoadLibraryExW(w!("crypt32.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
    let mut digest = [0; 32];
    let mut size = digest.len() as u32;
    let hashed =
        unsafe { GetProcAddress(module, s!("CryptHashCertificate2")) }.is_some_and(|entry| {
            let hash = unsafe { std::mem::transmute::<_, Hash>(entry) };
            unsafe {
                hash(
                    w!("SHA256"),
                    0,
                    std::ptr::null(),
                    input.as_ptr(),
                    input.len() as u32,
                    digest.as_mut_ptr(),
                    &mut size,
                )
            }
            .as_bool()
        });
    let _ = unsafe { FreeLibrary(module) };
    (hashed && size == digest.len() as u32).then_some(digest)
}

#[cfg(not(windows))]
fn sha256(_: &[u8]) -> Option<[u8; 32]> {
    None
}

fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn words_match(input: &[u8], offset: usize, expected: &[u32]) -> bool {
    expected
        .iter()
        .enumerate()
        .all(|(i, &value)| word(input, offset + i * 4) == value)
}

fn store_word(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_words(output: &mut [u8], offset: usize, values: &[u32]) -> usize {
    for (i, &value) in values.iter().enumerate() {
        store_word(output, offset + i * 4, value);
    }
    offset + values.len() * 4
}

fn checksum_matches(input: &[u8]) -> bool {
    dxbc_checksum(&input[20..ORIGINAL_SIZE])
        .is_some_and(|checksum| dxbc_digest_bytes(checksum) == input[4..20])
}

fn layout_matches(input: &[u8]) -> bool {
    word(input, 20) == 1
        && word(input, 28) == 3
        && word(input, 32) == 44
        && word(input, 36) == 156
        && word(input, 40) == SHADER_OFFSET as u32
        && &input[44..48] == b"ISGN"
        && word(input, 48) == 104
        && &input[156..160] == b"OSGN"
        && word(input, 160) == 44
        && &input[SHADER_OFFSET..SHADER_OFFSET + 4] == b"SHEX"
        && word(input, SHADER_OFFSET + 4) == SHADER_SIZE as u32
        && checksum_matches(input)
        && word(input, TOKENS) == 0x50
        && word(input, TOKENS + 4) == (SHADER_SIZE / 4) as u32
        && word(input, TEMPS) == 0x0200_0068
        && word(input, TEMPS + 4) == 14
        && words_match(input, PHASE_SPLIT, &NOISE)
        && words_match(input, START_SPLIT, &RAY_START)
}

fn rewrite(input: &[u8], output: &mut [u8]) -> bool {
    output[..PHASE_SPLIT].copy_from_slice(&input[..PHASE_SPLIT]);
    // Supply the verified View phase and keep the original phase/noise instructions.
    let noise_at = put_words(output, PHASE_SPLIT, &PHASE);
    let after_noise = noise_at + START_SPLIT - PHASE_SPLIT;
    output[noise_at..after_noise].copy_from_slice(&input[PHASE_SPLIT..START_SPLIT]);
    // Sample the four neighbouring texels for the depth bias, then replace the ray start.
    let cursor = put_words(output, after_noise, &BIAS);
    let cursor = put_words(output, cursor, &BIASED_START);
    // Keep the engine's trailer outside the DXBC container.
    output[cursor..].copy_from_slice(&input[START_SPLIT + RAY_START.len() * 4..]);

    let container_size = ORIGINAL_SIZE + GROWTH;
    let shader_size = SHADER_SIZE + GROWTH;
    store_word(output, 24, container_size as u32);
    store_word(output, SHADER_OFFSET + 4, shader_size as u32);
    store_word(output, TOKENS + 4, (shader_size / 4) as u32);
    store_word(output, TEMPS + 4, 18);
    // The noise dot product reads r14.xy in place of v2.xy.
    store_word(output, noise_at + 3 * 4, 0x0010_0046);
    store_word(output, noise_at + 4 * 4, 14);
    let Some(checksum) = dxbc_checksum(&output[20..container_size]) else {
        return false;
    };
    output[4..20].copy_from_slice(&dxbc_digest_bytes(checksum));
    true
}

/// The native caller lends the input and, on successful resize, a nonoverlapping output span.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_contact_shadow_correct(
    bytes: *const u8,
    size: usize,
    resize: Resize,
    user: *mut c_void,
) -> i32 {
    if bytes.is_null() || !(ORIGINAL_SIZE..=MAX_INPUT).contains(&size) {
        return UNMATCHED;
    }
    let input = unsafe { slice::from_raw_parts(bytes, size) };
    if &input[..4] != b"DXBC" || word(input, 24) != ORIGINAL_SIZE as u32 {
        return UNMATCHED;
    }
    if sha256(&input[..ORIGINAL_SIZE]) != Some(EXPECTED) {
        return UNMATCHED;
    }
    if !layout_matches(input) {
        return REFUSED;
    }
    let size = size + GROWTH;
    let output = unsafe { resize(user, size) };
    if output.is_null() {
        return REFUSED;
    }
    let output = unsafe { slice::from_raw_parts_mut(output, size) };
    if rewrite(input, output) {
        PATCHED
    } else {
        REFUSED
    }
}

/// CRC-32C of the declared DXBC container, ignoring any engine trailer. Input shorter than the
/// header or than its declared size hashes to zero.
pub(crate) fn container_crc(data: &[u8]) -> u32 {
    if data.len() < 28 {
        return 0;
    }
    data.get(..word(data, 24) as usize).map_or(0, crc32c)
}

/// The caller lends the shader bytes, including any engine trailer.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_contact_shadow_crc(bytes: *const u8, size: usize) -> u32 {
    if bytes.is_null() || size < 28 || size > isize::MAX as usize {
        return 0;
    }
    container_crc(unsafe { slice::from_raw_parts(bytes, size) })
}
