// SPDX-License-Identifier: MIT
//! The hash-pinned AC7 contact-shadow rewrite. The factory borrows the output for the call.
//! The shader facts and operands come from the existing capture evidence.
use crate::dxbc_chunks::word;
use crate::shader_hash::{crc32c, dxbc_checksum, dxbc_digest_bytes};
use crate::sys::{
    RSF_AC7_CONTACT_PATCHED, RSF_AC7_CONTACT_REFUSED, RSF_AC7_CONTACT_UNMATCHED,
    rsf_ac7_contact_shader_fn,
};
use crate::velocity_container::store;
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

const UNMATCHED: i32 = RSF_AC7_CONTACT_UNMATCHED as i32;
const PATCHED: i32 = RSF_AC7_CONTACT_PATCHED as i32;
const REFUSED: i32 = RSF_AC7_CONTACT_REFUSED as i32;

const EXPECTED: &[u8; 64] = b"60a2ca61e5d83b853f692ee8c0bc3580ea46b8e33d72c644949e5f37c1060a12";

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

fn words_match(input: &[u8], offset: usize, expected: &[u32]) -> bool {
    expected
        .iter()
        .enumerate()
        .all(|(i, &value)| word(input, offset + i * 4) == Some(value))
}

fn put_words(output: &mut [u8], offset: usize, values: &[u32]) -> usize {
    for (i, &value) in values.iter().enumerate() {
        store(output, offset + i * 4, value);
    }
    offset + values.len() * 4
}

fn checksum_matches(input: &[u8]) -> bool {
    dxbc_checksum(&input[20..ORIGINAL_SIZE])
        .is_some_and(|checksum| dxbc_digest_bytes(checksum) == input[4..20])
}

fn layout_matches(input: &[u8]) -> bool {
    word(input, 20) == Some(1)
        && word(input, 28) == Some(3)
        && word(input, 32) == Some(44)
        && word(input, 36) == Some(156)
        && word(input, 40) == Some(SHADER_OFFSET as u32)
        && &input[44..48] == b"ISGN"
        && word(input, 48) == Some(104)
        && &input[156..160] == b"OSGN"
        && word(input, 160) == Some(44)
        && &input[SHADER_OFFSET..SHADER_OFFSET + 4] == b"SHEX"
        && word(input, SHADER_OFFSET + 4) == Some(SHADER_SIZE as u32)
        && checksum_matches(input)
        && word(input, TOKENS) == Some(0x50)
        && word(input, TOKENS + 4) == Some((SHADER_SIZE / 4) as u32)
        && word(input, TEMPS) == Some(0x0200_0068)
        && word(input, TEMPS + 4) == Some(14)
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
    store(output, 24, container_size as u32);
    store(output, SHADER_OFFSET + 4, shader_size as u32);
    store(output, TOKENS + 4, (shader_size / 4) as u32);
    store(output, TEMPS + 4, 18);
    // The noise dot product reads r14.xy in place of v2.xy.
    store(output, noise_at + 3 * 4, 0x0010_0046);
    store(output, noise_at + 4 * 4, 14);
    let Some(checksum) = dxbc_checksum(&output[20..container_size]) else {
        return false;
    };
    output[4..20].copy_from_slice(&dxbc_digest_bytes(checksum));
    true
}

/// The native caller lends the input. On success the factory borrows Rust-owned output.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_contact_shadow_correct(
    bytes: *const u8,
    size: usize,
    offset: usize,
    use_shader: rsf_ac7_contact_shader_fn,
    user: *mut c_void,
) -> i32 {
    let Some(shader_size) = size.checked_sub(offset) else {
        return UNMATCHED;
    };
    if bytes.is_null() || size > MAX_INPUT || shader_size < ORIGINAL_SIZE {
        return UNMATCHED;
    }
    let source = unsafe { slice::from_raw_parts(bytes, size) };
    let (prefix, input) = source.split_at(offset);
    if &input[..4] != b"DXBC" || word(input, 24) != Some(ORIGINAL_SIZE as u32) {
        return UNMATCHED;
    }
    if rsf_windows::sha256_hex(&input[..ORIGINAL_SIZE]).as_ref() != Some(EXPECTED) {
        return UNMATCHED;
    }
    if !layout_matches(input) {
        return REFUSED;
    }
    let mut buffer = Vec::new();
    let Some(output) = crate::velocity_container::resize_bytes(&mut buffer, size + GROWTH) else {
        return REFUSED;
    };
    let (output_prefix, shader) = output.split_at_mut(offset);
    output_prefix.copy_from_slice(prefix);
    if !rewrite(input, shader) {
        return REFUSED;
    }
    let crc = container_crc(shader);
    if use_shader.is_some_and(|apply| unsafe { apply(user, output.as_ptr(), output.len(), crc) }) {
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
    word(data, 24)
        .and_then(|size| data.get(..size as usize))
        .map_or(0, crc32c)
}
