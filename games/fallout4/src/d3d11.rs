// SPDX-License-Identifier: MIT

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_FORMAT_SUPPORT_RENDER_TARGET,
    D3D11_FORMAT_SUPPORT_SHADER_LOAD, D3D11_FORMAT_SUPPORT_TEXTURE2D, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R11G11B10_FLOAT,
    DXGI_FORMAT_R16_FLOAT, DXGI_FORMAT_R16G16_FLOAT, DXGI_FORMAT_R16G16B16A16_FLOAT,
    DXGI_FORMAT_R24G8_TYPELESS, DXGI_FORMAT_R32_FLOAT,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::{IUnknown, Interface};

pub(crate) fn identity<T: Interface>(object: &T) -> u64 {
    object
        .cast::<IUnknown>()
        .map_or(0, |identity| identity.as_raw() as u64)
}

pub(crate) fn same_native(object: u64, native: u64) -> bool {
    if object == 0 || native == 0 {
        return false;
    }
    let raw = native as *mut c_void;
    // SAFETY: native names an interface on the current renderer or a leased target.
    unsafe { IUnknown::from_raw_borrowed(&raw) }.is_some_and(|native| identity(native) == object)
}

/// Both interfaces name the same COM object.
pub(crate) fn same_object(a: &impl Interface, b: &impl Interface) -> bool {
    let Ok(a) = a.cast::<IUnknown>() else {
        return false;
    };
    b.cast::<IUnknown>().is_ok_and(|b| a.as_raw() == b.as_raw())
}

/// Both nonnull pointers name the same COM object.
///
/// # Safety
/// Each pointer is null or a live interface.
pub(crate) unsafe fn same_raw(a: *mut c_void, b: *mut c_void) -> bool {
    match unsafe {
        (
            IUnknown::from_raw_borrowed(&a),
            IUnknown::from_raw_borrowed(&b),
        )
    } {
        (Some(a), Some(b)) => same_object(a, b),
        _ => false,
    }
}

pub(crate) fn viewport(context: &ID3D11DeviceContext) -> (u32, u32, [f32; 6]) {
    let mut count = 0;
    // SAFETY: the typed context is live. Request the count before reading a single viewport.
    unsafe { context.RSGetViewports(&mut count, None) };
    if count != 1 {
        return (count, 0, [0.0; 6]);
    }
    let mut viewport = D3D11_VIEWPORT::default();
    let mut copied = 1;
    unsafe { context.RSGetViewports(&mut copied, Some(&mut viewport)) };
    (
        count,
        copied,
        [
            viewport.TopLeftX,
            viewport.TopLeftY,
            viewport.Width,
            viewport.Height,
            viewport.MinDepth,
            viewport.MaxDepth,
        ],
    )
}

/// An immediate Context on the owner device, the chain on that device and the chain's output
/// window.
pub(crate) fn owner_matches(
    context: &ID3D11DeviceContext,
    chain: &IDXGISwapChain,
    expected_device: usize,
    expected_window: usize,
) -> bool {
    let device = unsafe { context.GetDevice() };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return false;
    }
    let Ok(device) = device else { return false };
    if !same_native(identity(&device), expected_device as u64) {
        return false;
    }
    let Ok(chain_device) = (unsafe { chain.GetDevice::<ID3D11Device>() }) else {
        return false;
    };
    same_object(&device, &chain_device)
        && unsafe { chain.GetDesc() }
            .is_ok_and(|description| description.OutputWindow.0 as usize == expected_window)
}

fn texture_desc(texture: &ID3D11Texture2D) -> D3D11_TEXTURE2D_DESC {
    let mut description = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut description) };
    description
}

pub(crate) fn framebuffer_extent(texture: &ID3D11Texture2D) -> [u32; 3] {
    let desc = texture_desc(texture);
    [desc.Width, desc.Height, desc.SampleDesc.Count]
}

pub(crate) fn auxiliary_depth(texture: &ID3D11Texture2D) -> bool {
    texture_desc(texture).Format == DXGI_FORMAT_R24G8_TYPELESS
}

/// A single-sample, single-mip default-usage colour target of the output extent on the Context's
/// device, in a format the host can load and render.
pub(crate) fn auxiliary_colour(
    texture: &ID3D11Texture2D,
    context: &ID3D11DeviceContext,
    width: u32,
    height: u32,
) -> bool {
    let device = unsafe { context.GetDevice() };
    let owner = unsafe { texture.GetDevice() };
    let desc = texture_desc(texture);
    let (Ok(device), Ok(owner)) = (device, owner) else {
        return false;
    };
    if !same_object(&device, &owner)
        || desc.Width != width
        || desc.Height != height
        || desc.MipLevels != 1
        || desc.ArraySize != 1
        || desc.SampleDesc.Count != 1
        || desc.SampleDesc.Quality != 0
        || desc.Usage != D3D11_USAGE_DEFAULT
    {
        return false;
    }
    let required = (D3D11_FORMAT_SUPPORT_TEXTURE2D.0
        | D3D11_FORMAT_SUPPORT_SHADER_LOAD.0
        | D3D11_FORMAT_SUPPORT_RENDER_TARGET.0) as u32;
    unsafe { device.CheckFormatSupport(desc.Format) }
        .is_ok_and(|support| support & required == required)
        && matches!(
            desc.Format,
            DXGI_FORMAT_R8_UNORM
                | DXGI_FORMAT_R8G8B8A8_UNORM
                | DXGI_FORMAT_R16G16_FLOAT
                | DXGI_FORMAT_R16_FLOAT
                | DXGI_FORMAT_R32_FLOAT
                | DXGI_FORMAT_R16G16B16A16_FLOAT
                | DXGI_FORMAT_R11G11B10_FLOAT
        )
}
