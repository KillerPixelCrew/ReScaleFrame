// SPDX-License-Identifier: MIT
//! The device fixtures' texture readback: rows packed into `output`, through `staging` when the fixture keeps one.
use std::ptr::copy_nonoverlapping;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// Copies `source` (one mip, one layer, one sample, of the formats the fixtures read) into `output`, whose length must
/// be the packed texture size. False on any mismatch or D3D11 failure.
pub(crate) fn readback(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    source: &ID3D11Texture2D,
    staging: Option<&ID3D11Texture2D>,
    output: &mut [u8],
) -> bool {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    if desc.MipLevels != 1 || desc.ArraySize != 1 || desc.SampleDesc.Count != 1 {
        return false;
    }
    let texel_bytes = match desc.Format {
        DXGI_FORMAT_R8G8B8A8_UNORM
        | DXGI_FORMAT_R16G16_FLOAT
        | DXGI_FORMAT_D32_FLOAT
        | DXGI_FORMAT_D24_UNORM_S8_UINT
        | DXGI_FORMAT_R24G8_TYPELESS => 4,
        DXGI_FORMAT_R32G32_FLOAT | DXGI_FORMAT_R16G16B16A16_FLOAT => 8,
        _ => return false,
    };
    let row_size = desc.Width as usize * texel_bytes;
    if row_size.checked_mul(desc.Height as usize) != Some(output.len()) {
        return false;
    }

    let allocated;
    let staging = if let Some(staging) = staging {
        let mut existing = D3D11_TEXTURE2D_DESC::default();
        unsafe { staging.GetDesc(&mut existing) };
        if existing.Width != desc.Width
            || existing.Height != desc.Height
            || existing.Format != desc.Format
            || existing.MipLevels != 1
            || existing.ArraySize != 1
            || existing.SampleDesc.Count != 1
            || existing.Usage != D3D11_USAGE_STAGING
            || existing.CPUAccessFlags & D3D11_CPU_ACCESS_READ.0 as u32 == 0
        {
            return false;
        }
        staging
    } else {
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut texture = None;
        if unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.is_err() {
            return false;
        }
        let Some(texture) = texture else {
            return false;
        };
        allocated = texture;
        &allocated
    };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { context.CopyResource(staging, source) };
    if unsafe { context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.is_err() {
        return false;
    }
    let pitch = mapped.RowPitch as usize;
    let valid = !mapped.pData.is_null() && row_size <= pitch;
    unsafe {
        if valid {
            for row in 0..desc.Height as usize {
                copy_nonoverlapping(
                    mapped.pData.cast::<u8>().add(row * pitch),
                    output.as_mut_ptr().add(row * row_size),
                    row_size,
                );
            }
        }
        context.Unmap(staging, 0);
    }
    valid
}
