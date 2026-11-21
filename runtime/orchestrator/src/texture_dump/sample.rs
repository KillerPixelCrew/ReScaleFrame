// SPDX-License-Identifier: MIT
use crate::dxgi_format::{
    D24_UNORM_S8_UINT, D32_FLOAT, D32_FLOAT_S8X24_UINT, R24G8_TYPELESS, R32_TYPELESS,
    R32G8X24_TYPELESS, half_to_f32,
};

const VELOCITY: u32 = 1;
const DECODED_MOTION: u32 = 2;
const HDR: u32 = 3;
const DEPTH: u32 = 4;
const SCALAR: u32 = 5;
const ALPHA: u32 = 6;
const DENSE_MOTION: u32 = 7;

// These parameters implement the existing ABI view when explicitly requested.
const VELOCITY_SCALE: f32 = 0.499 * 0.5;
const VELOCITY_BIAS: f32 = 32767.0 / 65535.0;
const DECODED_SENTINEL_THRESHOLD: f32 = -100.0;

#[derive(Clone, Copy)]
pub(super) enum Format {
    R8,
    Rg8,
    R16Float,
    R16Unorm,
    Depth24,
    R32Float,
    Depth32Stencil,
    Rgb10,
    Rgba8,
    Bgra8,
    Rg16Unorm,
    Rg16Float,
    Rg32Float,
    Rgb11Float,
    Rgba16Float,
    Rgba32Float,
}

impl Format {
    // Values from DXGI_FORMAT in texture_dump.cpp's compile-time checks.
    pub(super) fn from_dxgi(dxgi: u32) -> Option<Self> {
        Some(match dxgi {
            60 | 61 => Self::R8,
            49 => Self::Rg8,
            54 => Self::R16Float,
            53 | 56 => Self::R16Unorm,
            R24G8_TYPELESS | D24_UNORM_S8_UINT => Self::Depth24,
            R32_TYPELESS | D32_FLOAT | 41 => Self::R32Float,
            R32G8X24_TYPELESS | D32_FLOAT_S8X24_UINT => Self::Depth32Stencil,
            23 | 24 => Self::Rgb10,
            27..=29 => Self::Rgba8,
            87 | 90 | 91 => Self::Bgra8,
            35 => Self::Rg16Unorm,
            34 => Self::Rg16Float,
            16 => Self::Rg32Float,
            26 => Self::Rgb11Float,
            10 => Self::Rgba16Float,
            2 => Self::Rgba32Float,
            _ => return None,
        })
    }

    pub(super) fn bytes(self) -> u32 {
        match self {
            Self::R8 => 1,
            Self::Rg8 | Self::R16Float | Self::R16Unorm => 2,
            Self::Depth24
            | Self::R32Float
            | Self::Rgb10
            | Self::Rgba8
            | Self::Bgra8
            | Self::Rg16Unorm
            | Self::Rg16Float
            | Self::Rgb11Float => 4,
            Self::Rgba16Float | Self::Rg32Float | Self::Depth32Stencil => 8,
            Self::Rgba32Float => 16,
        }
    }

    pub(super) fn decode(self, pixel: &[u8]) -> Sample {
        let byte = |index: usize| f32::from(pixel[index]) / 255.0;
        let word = |index: usize| {
            let offset = index * 2;
            u16::from_le_bytes([pixel[offset], pixel[offset + 1]])
        };
        let dword = |index: usize| {
            let offset = index * 4;
            u32::from_le_bytes(pixel[offset..offset + 4].try_into().expect("four bytes"))
        };
        let float = |index| f32::from_bits(dword(index));
        match self {
            Self::R8 => Sample::scalar(byte(0)),
            Self::Rg8 => Sample::pair(byte(0), byte(1), true),
            Self::R16Float => Sample::scalar(half_to_f32(word(0))),
            Self::R16Unorm => Sample::scalar(unorm16(word(0))),
            Self::Depth24 => Sample::scalar((dword(0) & 0xff_ffff) as f32 / 16_777_215.0),
            Self::R32Float | Self::Depth32Stencil => Sample::scalar(float(0)),
            Self::Rgb10 => {
                let value = dword(0);
                let channel = |shift: u32| ((value >> shift) & 1023) as f32 / 1023.0;
                Sample::new(
                    [
                        channel(0),
                        channel(10),
                        channel(20),
                        (value >> 30) as f32 / 3.0,
                    ],
                    true,
                )
            }
            Self::Rgba8 => Sample::new([byte(0), byte(1), byte(2), byte(3)], true),
            Self::Bgra8 => Sample::new([byte(2), byte(1), byte(0), byte(3)], true),
            Self::Rg16Unorm => {
                let (x, y) = (word(0), word(1));
                Sample::pair(unorm16(x), unorm16(y), x != 0)
            }
            Self::Rg16Float => {
                let (x, y) = (word(0), word(1));
                Sample::pair(half_to_f32(x), half_to_f32(y), x != 0 || y != 0)
            }
            Self::Rg32Float => {
                let (x, y) = (float(0), float(1));
                Sample::pair(x, y, x != 0.0 || y != 0.0)
            }
            Self::Rgb11Float => {
                let value = dword(0);
                Sample::new(
                    [
                        packed_float(value & 0x7ff, 6),
                        packed_float((value >> 11) & 0x7ff, 6),
                        packed_float(value >> 22, 5),
                        1.0,
                    ],
                    value != 0,
                )
            }
            Self::Rgba16Float => {
                let [x, y, z, a] = [word(0), word(1), word(2), word(3)];
                Sample::new([x, y, z, a].map(half_to_f32), x != 0 || y != 0 || z != 0)
            }
            Self::Rgba32Float => Sample::new([float(0), float(1), float(2), float(3)], true),
        }
    }
}

fn unorm16(value: u16) -> f32 {
    f32::from(value) / 65535.0
}

fn packed_float(value: u32, mantissa_bits: u32) -> f32 {
    if value >> mantissa_bits == 31 {
        return if value & ((1 << mantissa_bits) - 1) == 0 {
            65504.0
        } else {
            0.0
        };
    }
    // The unsigned fields share half's exponent and bias. Extend the mantissa.
    half_to_f32((value << (10 - mantissa_bits)) as u16)
}

pub(super) struct Sample {
    rgba: [f32; 4],
    written: bool,
}

impl Sample {
    fn new(rgba: [f32; 4], written: bool) -> Self {
        Self { rgba, written }
    }

    fn scalar(value: f32) -> Self {
        Self::new([value, value, value, 1.0], true)
    }

    fn pair(x: f32, y: f32, written: bool) -> Self {
        Self::new([x, y, 0.0, 1.0], written)
    }

    pub(super) fn channels(&self, view: u32) -> Option<(f32, f32)> {
        let [x, y, _, _] = self.rgba;
        match view {
            VELOCITY if !self.written => None,
            VELOCITY => Some((
                (x - VELOCITY_BIAS) / VELOCITY_SCALE,
                (y - VELOCITY_BIAS) / VELOCITY_SCALE,
            )),
            DECODED_MOTION if x <= DECODED_SENTINEL_THRESHOLD => None,
            _ => Some((x, y)),
        }
    }

    pub(super) fn bgra(&self, view: u32, scale: f32, red: f32, green: f32) -> [u8; 4] {
        if matches!(view, VELOCITY | DECODED_MOTION | DENSE_MOTION) {
            let motion = |value| clamp_byte(128.0 + value * scale * 127.0);
            return [128, motion(green), motion(red), 255];
        }
        let [_, _, blue, alpha] = self.rgba;
        let [red, green, blue] = match view {
            HDR => [red, green, blue].map(|value| display_hdr(value * scale)),
            DEPTH => {
                let depth = if red.is_finite() && red > 0.0 {
                    (1.0 + red * 65535.0).log2() / 16.0
                } else {
                    0.0
                };
                [depth; 3]
            }
            ALPHA => [alpha; 3],
            SCALAR => [red * scale; 3],
            _ => [red * scale, green * scale, blue * scale],
        };
        [
            clamp_byte(blue * 255.0),
            clamp_byte(green * 255.0),
            clamp_byte(red * 255.0),
            255,
        ]
    }
}

fn clamp_byte(value: f32) -> u8 {
    if value.is_finite() {
        (value + 0.5) as u8
    } else {
        0
    }
}

fn display_hdr(value: f32) -> f32 {
    let value = if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    };
    let value = value / (1.0 + value);
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) struct Range {
    min: f32,
    max: f32,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            min: f32::INFINITY,
            max: f32::NEG_INFINITY,
        }
    }
}

impl Range {
    pub(super) fn include(&mut self, value: f32) {
        if value.is_finite() {
            if value < self.min {
                self.min = value;
            }
            if value > self.max {
                self.max = value;
            }
        }
    }

    pub(super) fn finish(self) -> [f32; 2] {
        if self.min > self.max {
            [0.0; 2]
        } else {
            [self.min, self.max]
        }
    }
}
