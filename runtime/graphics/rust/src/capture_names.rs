// SPDX-License-Identifier: MIT
use std::ffi::{CStr, CString};
use std::io::Write as _;

pub const HDR: u32 = 3;
pub const DEPTH: u32 = 4;
pub const SCALAR: u32 = 5;
pub const ALPHA: u32 = 6;
pub const DENSE_MOTION: u32 = 7;

pub struct CaptureName {
    pub name: CString,
    pub view: u32,
    pub scale: f32,
}

fn contains(text: &[u8], word: &[u8]) -> bool {
    text.windows(word.len()).any(|part| part == word)
}

impl CaptureName {
    // The capacity includes the terminator. Preserve the caller's role bytes
    // even when they do not form valid UTF-8.
    pub fn for_role(role: &CStr, ordinal: u32, capacity: usize) -> Option<Self> {
        let role = role.to_bytes();
        let stem: &[u8] = match role {
            b"sr-linear-color" => b"sr_color",
            b"sr-output-linear" => b"sr_output",
            b"fg-hudless-color" => b"hudless_color",
            b"fg-motion-pixels" | b"fg-motion-uv" => b"fg_motion",
            b"sr-motion-pixels" | b"sr-motion-uv" => b"sr_motion",
            b"sr-game-motion-uv" => b"sr_game_motion",
            _ => role,
        };
        let suffix = if ordinal == 0 {
            0
        } else {
            ordinal.ilog10() as usize + 2
        };
        let size = stem.len().checked_add(suffix + 1)?;
        if size > capacity {
            return None;
        }
        let mut name = Vec::new();
        name.try_reserve_exact(size).ok()?;
        name.extend(
            stem.iter()
                .map(|&byte| if byte == b'-' { b'_' } else { byte }),
        );
        if ordinal != 0 {
            write!(&mut name, "_{ordinal}").ok()?;
        }
        let view = if contains(&name, b"depth") {
            DEPTH
        } else if contains(&name, b"alpha") {
            ALPHA
        } else if contains(&name, b"motion") {
            DENSE_MOTION
        } else if contains(&name, b"mask") || contains(&name, b"exposure") {
            SCALAR
        } else {
            HDR
        };
        // The aliases omit the units. Scale from the original role.
        let scale = if !contains(role, b"motion") {
            1.0
        } else if contains(role, b"uv") {
            8.0
        } else {
            1.0 / 32.0
        };
        name.push(0);
        Some(Self {
            name: CString::from_vec_with_nul(name).ok()?,
            view,
            scale,
        })
    }
}
