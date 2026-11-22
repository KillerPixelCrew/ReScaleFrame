// SPDX-License-Identifier: MIT
//! Typed D3D11 leaves shared by the Rust render paths. The functions the orchestrator's platform tables point at keep the C calling convention.
use crate::sys::rsf_d3d11_texture_desc as Desc;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

type Raw = *mut c_void;

pub unsafe fn rsf_d3d11_output_query_texture(resource: Raw) -> Raw {
    unsafe { ID3D11Resource::from_raw_borrowed(&resource) }
        .and_then(|resource| resource.cast::<ID3D11Texture2D>().ok())
        .map_or(null_mut(), Interface::into_raw)
}

/// # Safety
///
/// `child` is null or a live `ID3D11DeviceChild`; the caller owns the returned device.
pub unsafe extern "C" fn rsf_d3d11_output_child_device(child: Raw) -> Raw {
    unsafe { ID3D11DeviceChild::from_raw_borrowed(&child) }
        .and_then(|child| unsafe { child.GetDevice().ok() })
        .map_or(null_mut(), Interface::into_raw)
}

pub unsafe extern "C" fn rsf_d3d11_output_texture_desc(texture: Raw, desc: *mut Desc) {
    let mut native = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        ID3D11Texture2D::from_raw_borrowed(&texture)
            .unwrap()
            .GetDesc(&mut native)
    };
    unsafe {
        desc.write(Desc {
            width: native.Width,
            height: native.Height,
            mips: native.MipLevels,
            layers: native.ArraySize,
            format: native.Format.0 as u32,
            samples: native.SampleDesc.Count,
            sample_quality: native.SampleDesc.Quality,
            usage: native.Usage.0 as u32,
            bind_flags: native.BindFlags,
            cpu_access_flags: native.CPUAccessFlags,
            misc_flags: native.MiscFlags,
        });
    }
}

pub unsafe extern "C" fn rsf_d3d11_output_create_texture(device: Raw, desc: *const Desc) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    let desc = unsafe { &*desc };
    let native = D3D11_TEXTURE2D_DESC {
        Width: desc.width,
        Height: desc.height,
        MipLevels: desc.mips,
        ArraySize: desc.layers,
        Format: DXGI_FORMAT(desc.format as i32),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: desc.samples,
            Quality: desc.sample_quality,
        },
        Usage: D3D11_USAGE(desc.usage as i32),
        BindFlags: desc.bind_flags,
        CPUAccessFlags: desc.cpu_access_flags,
        MiscFlags: desc.misc_flags,
    };
    let mut result = None;
    if unsafe { device.CreateTexture2D(&native, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    result.map_or(null_mut(), Interface::into_raw)
}

pub unsafe fn rsf_d3d11_output_create_srv(device: Raw, texture: Raw, format: u32) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture).unwrap() };
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT(format as i32),
        ViewDimension: D3D_SRV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_SRV {
                MostDetailedMip: 0,
                MipLevels: 1,
            },
        },
    };
    let mut result = None;
    if unsafe { device.CreateShaderResourceView(texture, Some(&desc), Some(&mut result)) }.is_err()
    {
        return null_mut();
    }
    result.map_or(null_mut(), Interface::into_raw)
}

/// # Safety
///
/// `device` must be a live `ID3D11Device` and `texture` a live resource of it; the caller owns
/// the returned view.
pub unsafe extern "C" fn rsf_d3d11_output_create_rtv(device: Raw, texture: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture).unwrap() };
    let mut result = None;
    if unsafe { device.CreateRenderTargetView(texture, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    result.map_or(null_mut(), Interface::into_raw)
}

pub fn rsf_d3d11_output_srv_format(format: u32) -> u32 {
    srv_format(DXGI_FORMAT(format as i32)).0 as u32
}

pub fn srv_format(format: DXGI_FORMAT) -> DXGI_FORMAT {
    match format {
        DXGI_FORMAT_R32G8X24_TYPELESS | DXGI_FORMAT_D32_FLOAT_S8X24_UINT => {
            DXGI_FORMAT_R32_FLOAT_X8X24_TYPELESS
        }
        DXGI_FORMAT_R24G8_TYPELESS | DXGI_FORMAT_D24_UNORM_S8_UINT => {
            DXGI_FORMAT_R24_UNORM_X8_TYPELESS
        }
        DXGI_FORMAT_R32_TYPELESS | DXGI_FORMAT_D32_FLOAT => DXGI_FORMAT_R32_FLOAT,
        DXGI_FORMAT_R16_TYPELESS | DXGI_FORMAT_D16_UNORM => DXGI_FORMAT_R16_UNORM,
        DXGI_FORMAT_R32G32B32A32_TYPELESS => DXGI_FORMAT_R32G32B32A32_FLOAT,
        DXGI_FORMAT_R16G16B16A16_TYPELESS => DXGI_FORMAT_R16G16B16A16_FLOAT,
        DXGI_FORMAT_R32G32_TYPELESS => DXGI_FORMAT_R32G32_FLOAT,
        DXGI_FORMAT_R16G16_TYPELESS => DXGI_FORMAT_R16G16_FLOAT,
        DXGI_FORMAT_R10G10B10A2_TYPELESS => DXGI_FORMAT_R10G10B10A2_UNORM,
        DXGI_FORMAT_R8G8B8A8_TYPELESS => DXGI_FORMAT_R8G8B8A8_UNORM,
        DXGI_FORMAT_B8G8R8A8_TYPELESS => DXGI_FORMAT_B8G8R8A8_UNORM,
        typed => typed,
    }
}

/// # Safety
///
/// `device` must be a live `ID3D11Device`.
pub unsafe extern "C" fn rsf_d3d11_output_device_ok(device: Raw) -> u32 {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device).unwrap() };
    unsafe { device.GetDeviceRemovedReason().is_ok() as u32 }
}

/// # Safety
///
/// `context` must be a live `ID3D11DeviceContext`.
pub unsafe extern "C" fn rsf_d3d11_output_is_immediate(context: Raw) -> u32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    unsafe { (context.GetType() == D3D11_DEVICE_CONTEXT_IMMEDIATE) as u32 }
}

pub unsafe fn rsf_d3d11_output_clear_state(context: Raw) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    unsafe { context.ClearState() };
}

pub unsafe fn rsf_d3d11_output_unbind_targets(context: Raw) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    unsafe { context.OMSetRenderTargets(None, None::<&ID3D11DepthStencilView>) };
}

pub unsafe fn rsf_d3d11_output_copy_resource(context: Raw, destination: Raw, source: Raw) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    let destination = unsafe { ID3D11Resource::from_raw_borrowed(&destination).unwrap() };
    let source = unsafe { ID3D11Resource::from_raw_borrowed(&source).unwrap() };
    unsafe { context.CopyResource(destination, source) };
}

pub unsafe fn rsf_d3d11_output_copy_region(
    context: Raw,
    destination: Raw,
    source: Raw,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context).unwrap() };
    let destination = unsafe { ID3D11Resource::from_raw_borrowed(&destination).unwrap() };
    let source = unsafe { ID3D11Resource::from_raw_borrowed(&source).unwrap() };
    let region = D3D11_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: width,
        bottom: height,
        back: 1,
    };
    unsafe {
        context.CopySubresourceRegion(destination, 0, left, top, 0, source, 0, Some(&region))
    };
}
