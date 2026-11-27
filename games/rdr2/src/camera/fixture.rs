// SPDX-License-Identifier: MIT
use super::CameraInputs;

pub(super) struct Fixture<'a> {
    pub(super) view: &'a [u8],
    pub(super) input: CameraInputs,
    pub(super) jitter_matches: bool,
    pub(super) fov: f32,
}

impl<'a> Fixture<'a> {
    pub(super) fn parse(bytes: &'a [u8]) -> Option<Self> {
        let bytes = bytes.strip_prefix(b"RSFCAMF1")?;
        let (view, fields) = bytes.split_at_checked(0x800)?;
        let fields = fields.get(..48)?;
        let word = |offset| u32::from_le_bytes(std::array::from_fn(|i| fields[offset + i]));
        let real = |offset| f32::from_bits(word(offset));
        Some(Self {
            view,
            input: CameraInputs {
                render_width: word(0),
                render_height: word(4),
                output_width: word(8),
                output_height: word(12),
                jitter: [real(16), real(20)],
                previous_jitter: [real(24), real(28)],
                frame_time_seconds: real(32),
                reset: fields[36] != 0,
                motion_jittered: fields[37] != 0,
            },
            jitter_matches: word(40) != 0,
            fov: real(44),
        })
    }
}

#[cfg(feature = "camera-fixture")]
mod driver;
