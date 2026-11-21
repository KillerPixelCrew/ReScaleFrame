// SPDX-License-Identifier: MIT
// DXGI depth format values shared by the D3D11 and D3D12 paths.
pub(super) const R32G8X24_TYPELESS: u32 = 19;
pub(super) const D32_FLOAT_S8X24_UINT: u32 = 20;
pub(super) const R32_TYPELESS: u32 = 39;
pub(super) const D32_FLOAT: u32 = 40;
pub(super) const R24G8_TYPELESS: u32 = 44;
pub(super) const D24_UNORM_S8_UINT: u32 = 45;
pub(super) const R24_UNORM_X8_TYPELESS: u32 = 46;

pub(super) fn half_to_f32(half: u16) -> f32 {
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
