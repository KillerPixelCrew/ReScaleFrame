// SPDX-License-Identifier: MIT
// DXGI format values shared by the Direct3D paths.
pub const RGBA32_FLOAT: u32 = 2;
pub const RGBA16_FLOAT: u32 = 10;
pub const R32G8X24_TYPELESS: u32 = 19;
pub const D32_FLOAT_S8X24_UINT: u32 = 20;
pub const R32_TYPELESS: u32 = 39;
pub const D32_FLOAT: u32 = 40;
pub const R24G8_TYPELESS: u32 = 44;
pub const D24_UNORM_S8_UINT: u32 = 45;
pub const R24_UNORM_X8_TYPELESS: u32 = 46;

pub fn half_to_f32(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exponent = u32::from((half >> 10) & 0x1f);
    let mantissa = u32::from(half & 0x3ff);
    match exponent {
        0 => {
            let magnitude = mantissa as f32 / 16_777_216.0;
            if sign != 0 { -magnitude } else { magnitude }
        }
        0x1f => f32::from_bits(sign | 0x7f80_0000 | (mantissa << 13)),
        _ => f32::from_bits(sign | ((exponent + 112) << 23) | (mantissa << 13)),
    }
}
