// SPDX-License-Identifier: MIT
//! Lossless saved text in a Windows profile, including existing ANSI files.

const PREFIX: &str = "@RSF-UTF16:";
const HEX: &[u8; 16] = b"0123456789abcdef";

/// Maximum number of UTF-16 units accepted for a new saved text edit.
pub const MAX_UNITS: usize = 4096;

/// Buffer capacity for an existing unencoded value or a saved encoded edit.
pub fn read_capacity(capacity: usize) -> usize {
    capacity.max((capacity.saturating_sub(1).min(MAX_UNITS)) * 4 + PREFIX.len() + 1)
}

/// Encodes saved text independently of the existing INI file's code page
/// and its treatment of literal quotes and whitespace.
pub fn encode(units: &[u16]) -> Vec<u16> {
    if units.is_empty() {
        return Vec::new();
    }
    let mut output: Vec<_> = PREFIX.encode_utf16().collect();
    output.reserve(units.len() * 4);
    for &unit in units {
        for shift in [12, 8, 4, 0] {
            output.push(u16::from(HEX[usize::from((unit >> shift) & 15)]));
        }
    }
    output
}

/// Decodes a saved value. A legacy value or malformed prefix is preserved.
pub fn decode(units: Vec<u16>) -> Vec<u16> {
    let prefix: Vec<_> = PREFIX.encode_utf16().collect();
    let Some(encoded) = units.strip_prefix(prefix.as_slice()) else {
        return units;
    };
    if encoded.len() % 4 != 0 {
        return units;
    }
    let decoded: Option<Vec<u16>> = encoded
        .chunks_exact(4)
        .map(|group| {
            group.iter().try_fold(0u16, |value, &digit| {
                let digit = u8::try_from(digit).ok()?;
                let nibble = HEX.iter().position(|&ch| ch == digit)?;
                Some((value << 4) | nibble as u16)
            })
        })
        .collect();
    decoded
        .filter(|text| !text.contains(&0) && String::from_utf16(text).is_ok())
        .unwrap_or(units)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_supplementary_characters_as_ascii_and_keeps_legacy_text() {
        let path: Vec<_> = "C:\\試験\\🧪".encode_utf16().collect();
        let encoded = encode(&path);
        assert!(encoded.iter().all(|&unit| unit <= 127));
        assert_eq!(decode(encoded), path);
        assert_eq!(decode(path.clone()), path);
        let prefix: Vec<_> = "@RSF-UTF16:0061".encode_utf16().collect();
        assert_eq!(decode(encode(&prefix)), prefix);
    }
}
