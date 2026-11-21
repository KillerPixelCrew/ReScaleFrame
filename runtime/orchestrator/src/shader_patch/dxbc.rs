// SPDX-License-Identifier: MIT
//! Synthetic-only DXBC operation: one exact, same-length instruction replacement.

use super::{ANCHOR, ARGUMENT, FORMAT, HASH, Outcome, check, refuse, sha256};
use crate::dxbc_chunks;
use crate::shader_hash::{dxbc_checksum, dxbc_digest_bytes};

#[cfg(test)]
pub(super) use crate::shader_hash::MD5_K as K;
const CUSTOM_DATA: u32 = 53;

fn word(s: &[u8], at: usize) -> Outcome<u32> {
    dxbc_chunks::word(s, at).ok_or_else(|| refuse(FORMAT, "DXBC word"))
}

pub(super) fn checksum(payload: &[u8]) -> Outcome<[u32; 4]> {
    check(payload.len().is_multiple_of(4), FORMAT, "DXBC alignment")?;
    dxbc_checksum(payload).ok_or_else(|| refuse(FORMAT, "DXBC alignment"))
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
    let stored = dxbc_digest_bytes(checksum(&input[20..])?);
    check(stored == input[4..20], FORMAT, "DXBC checksum")?;
    let chunks = word(input, 28)? as usize;
    check(chunks <= (input.len() - 32) / 4, FORMAT, "DXBC chunks")?;
    let pattern: Vec<u8> = instruction.iter().flat_map(|t| t.to_le_bytes()).collect();
    let mut found = None;
    let mut count = 0;
    for i in 0..chunks {
        let at = word(input, 32 + i * 4)? as usize;
        let chunk = dxbc_chunks::chunk_at(input, 32 + chunks * 4, at)
            .ok_or_else(|| refuse(FORMAT, "DXBC chunk size"))?;
        let size = chunk.data.len();
        let kind = chunk.tag;
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
    let digest = dxbc_digest_bytes(checksum(&output[20..])?);
    output[4..20].copy_from_slice(&digest);
    Ok(output)
}
