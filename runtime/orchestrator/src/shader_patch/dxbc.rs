// SPDX-License-Identifier: MIT
//! Synthetic-only DXBC operation: one exact, same-length instruction replacement.

use super::{ANCHOR, ARGUMENT, FORMAT, HASH, Outcome, check, refuse, sha256};

// RFC 1321 round constants and shifts.
pub(super) const K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];
const SHIFTS: [[u32; 4]; 4] = [
    [7, 12, 17, 22],
    [5, 9, 14, 20],
    [4, 11, 16, 23],
    [6, 10, 15, 21],
];
const CUSTOM_DATA: u32 = 53;

fn word(s: &[u8], at: usize) -> Outcome<u32> {
    let bytes = s.get(at..at.saturating_add(4)).filter(|b| b.len() == 4);
    let bytes = bytes.ok_or_else(|| refuse(FORMAT, "DXBC word"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap_or_default()))
}

/// MD5 compression over the container after its checksum, with DXBC's final-block layout:
/// the bit length leads a short tail, and every digest ends with `2 * bytes + 1`.
pub(super) fn checksum(s: &[u8]) -> Outcome<[u32; 4]> {
    check(
        s.len().is_multiple_of(4) && s.len() <= u32::MAX as usize,
        FORMAT,
        "DXBC alignment",
    )?;
    let mut data: Vec<u32> = s
        .chunks_exact(4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .collect();
    let full = data.len() / 16 * 16;
    let length = s.len() as u32;
    if data.len() - full < 14 {
        data.insert(full, length << 3);
        data.push(0x80);
        data.resize(full + 15, 0);
    } else {
        data.push(0x80);
        data.resize(full + 16, 0);
        data.push(length << 3);
        data.resize(full + 31, 0);
    }
    data.push(length.wrapping_mul(2).wrapping_add(1));
    let mut h = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    for block in data.chunks_exact(16) {
        let [mut a, mut b, mut c, mut d] = h;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let sum = a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(block[g]);
            let next = b.wrapping_add(sum.rotate_left(SHIFTS[i / 16][i % 4]));
            (a, d, c, b) = (d, c, b, next);
        }
        for (state, value) in h.iter_mut().zip([a, b, c, d]) {
            *state = state.wrapping_add(value);
        }
    }
    Ok(h)
}

fn digest_bytes(h: [u32; 4]) -> [u8; 16] {
    let mut out = [0; 16];
    for (bytes, value) in out.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
    out
}

/// Replaces the single matching instruction of a hash-pinned container and repairs its checksum.
pub(super) fn rewrite(
    input: &[u8],
    expected_sha256: &[u8],
    instruction: &[u32],
    replacement: &[u32],
) -> Outcome<Vec<u8>> {
    check(
        sha256(input)?.as_bytes() == expected_sha256,
        HASH,
        "DXBC hash",
    )?;
    let header = input.len() >= 32 && &input[..4] == b"DXBC";
    check(
        header && word(input, 24)? as usize == input.len(),
        FORMAT,
        "DXBC header",
    )?;
    let length = |tokens: &[u32]| tokens.first().map(|t| (t >> 24 & 127) as usize);
    let whole = !instruction.is_empty()
        && instruction.len() == replacement.len()
        && length(instruction) == Some(instruction.len())
        && length(replacement) == Some(replacement.len());
    check(whole, ARGUMENT, "whole same-size instruction required")?;
    let stored = digest_bytes(checksum(&input[20..])?);
    check(stored == input[4..20], FORMAT, "DXBC checksum")?;
    let chunks = word(input, 28)? as usize;
    check(chunks <= (input.len() - 32) / 4, FORMAT, "DXBC chunks")?;
    let pattern: Vec<u8> = instruction.iter().flat_map(|t| t.to_le_bytes()).collect();
    let mut found = None;
    let mut count = 0;
    for i in 0..chunks {
        let at = word(input, 32 + i * 4)? as usize;
        let size = word(input, at + 4)? as usize;
        let bounded =
            at >= 32 + chunks * 4 && at + 8 <= input.len() && size <= input.len() - at - 8;
        check(bounded, FORMAT, "DXBC chunk size")?;
        let kind = &input[at..at + 4];
        if kind != b"SHDR" && kind != b"SHEX" {
            continue;
        }
        let program =
            size >= 8 && size.is_multiple_of(4) && word(input, at + 12)? as usize == size / 4;
        check(program, FORMAT, "token program length")?;
        let end = at + 8 + size;
        let mut pos = at + 16;
        while pos < end {
            let opcode = word(input, pos)?;
            let mut n = (opcode >> 24 & 127) as usize;
            if opcode & 0x7ff == CUSTOM_DATA {
                n = word(input, pos + 4)? as usize;
            }
            check(n != 0 && n <= (end - pos) / 4, FORMAT, "instruction length")?;
            if input[pos..pos + n * 4] == pattern[..] {
                found = Some(pos);
                count += 1;
            }
            pos += n * 4;
        }
    }
    let at = found
        .filter(|_| count == 1)
        .ok_or_else(|| refuse(ANCHOR, "missing or ambiguous DXBC instruction"))?;
    let mut output = input.to_vec();
    for (i, token) in replacement.iter().enumerate() {
        output[at + i * 4..at + i * 4 + 4].copy_from_slice(&token.to_le_bytes());
    }
    let digest = digest_bytes(checksum(&output[20..])?);
    output[4..20].copy_from_slice(&digest);
    Ok(output)
}
