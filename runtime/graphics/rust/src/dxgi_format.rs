// SPDX-License-Identifier: MIT
// DXGI format values shared by the Direct3D paths, as the u32 the records carry.
use windows::Win32::Graphics::Dxgi::Common::*;

pub const RGBA32_FLOAT: u32 = DXGI_FORMAT_R32G32B32A32_FLOAT.0 as u32;
pub const RGBA16_FLOAT: u32 = DXGI_FORMAT_R16G16B16A16_FLOAT.0 as u32;
pub const R32G8X24_TYPELESS: u32 = DXGI_FORMAT_R32G8X24_TYPELESS.0 as u32;
pub const D32_FLOAT_S8X24_UINT: u32 = DXGI_FORMAT_D32_FLOAT_S8X24_UINT.0 as u32;
pub const R32_TYPELESS: u32 = DXGI_FORMAT_R32_TYPELESS.0 as u32;
pub const D32_FLOAT: u32 = DXGI_FORMAT_D32_FLOAT.0 as u32;
pub const R24G8_TYPELESS: u32 = DXGI_FORMAT_R24G8_TYPELESS.0 as u32;
pub const D24_UNORM_S8_UINT: u32 = DXGI_FORMAT_D24_UNORM_S8_UINT.0 as u32;
pub const R24_UNORM_X8_TYPELESS: u32 = DXGI_FORMAT_R24_UNORM_X8_TYPELESS.0 as u32;

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
