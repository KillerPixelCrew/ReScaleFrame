// SPDX-License-Identifier: MIT
use super::Raw;
use crate::ffi::borrow_span;
use super::Targets;
use std::ffi::{CStr, c_char};
use std::mem::MaybeUninit;
use std::ptr::null;
use windows::Win32::Foundation::TRUE;
use windows::Win32::Graphics::Direct3D::{
    D3D_REGISTER_COMPONENT_FLOAT32, D3D_REGISTER_COMPONENT_UINT32,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::core::Interface;

fn texture(
    device: &ID3D11Device,
    size: u32,
    format: DXGI_FORMAT,
    bind: D3D11_BIND_FLAG,
) -> Option<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: size,
        Height: size,
        MipLevels: 1,
        ArraySize: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: bind.0 as u32,
        ..Default::default()
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    texture
}

fn colour(
    device: &ID3D11Device,
    size: u32,
    format: DXGI_FORMAT,
) -> Option<(ID3D11Texture2D, ID3D11RenderTargetView)> {
    let texture = texture(
        device,
        size,
        format,
        D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE,
    )?;
    let mut view = None;
    unsafe { device.CreateRenderTargetView(&texture, None, Some(&mut view)) }.ok()?;
    Some((texture, view?))
}

fn targets(device: &ID3D11Device, size: u32, stencil: bool) -> Result<Targets, &'static CStr> {
    let (position, position_rtv) =
        colour(device, size, DXGI_FORMAT_R16G16B16A16_FLOAT).ok_or(c"position target failed")?;
    let (albedo, albedo_rtv) =
        colour(device, size, DXGI_FORMAT_R8G8B8A8_UNORM).ok_or(c"albedo target failed")?;
    let depth_format = if stencil {
        DXGI_FORMAT_D24_UNORM_S8_UINT
    } else {
        DXGI_FORMAT_D32_FLOAT
    };
    let depth =
        texture(device, size, depth_format, D3D11_BIND_DEPTH_STENCIL).ok_or(c"depth failed")?;
    let mut depth_view = None;
    unsafe { device.CreateDepthStencilView(&depth, None, Some(&mut depth_view)) }
        .map_err(|_| c"dsv failed")?;
    let depth_view = depth_view.ok_or(c"dsv failed")?;
    let state = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: TRUE,
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
        DepthFunc: if stencil {
            D3D11_COMPARISON_LESS_EQUAL
        } else {
            D3D11_COMPARISON_LESS
        },
        ..Default::default()
    };
    let mut depth_state = None;
    unsafe { device.CreateDepthStencilState(&state, Some(&mut depth_state)) }
        .map_err(|_| c"depth state failed")?;
    let depth_state = depth_state.ok_or(c"depth state failed")?;
    let state = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: TRUE,
        ..Default::default()
    };
    let mut raster = None;
    unsafe { device.CreateRasterizerState(&state, Some(&mut raster)) }
        .map_err(|_| c"raster failed")?;
    let raster = raster.ok_or(c"raster failed")?;
    Ok(Targets {
        position_texture: position.into_raw(),
        position_rtv: position_rtv.into_raw(),
        albedo_texture: albedo.into_raw(),
        albedo_rtv: albedo_rtv.into_raw(),
        depth_texture: depth.into_raw(),
        depth_view: depth_view.into_raw(),
        depth_state: depth_state.into_raw(),
        raster: raster.into_raw(),
    })
}

pub(in crate::motion::device_fixture) unsafe fn rsf_anomaly_motion_fixture_make_targets(
    device: Raw,
    size: u32,
    stencil: bool,
    out: &mut MaybeUninit<Targets>,
) -> *const c_char {
    let Some(device) = (unsafe { ID3D11Device::from_raw_borrowed(&device) }) else {
        return c"fixture device missing".as_ptr();
    };
    match targets(device, size, stencil) {
        Err(error) => error.as_ptr(),
        Ok(targets) => {
            out.write(targets);
            null()
        }
    }
}

pub(in crate::motion::device_fixture) unsafe fn rsf_anomaly_motion_fixture_make_layout(
    device: Raw,
    signature: *const D3D11_SIGNATURE_PARAMETER_DESC,
    count: u32,
    stock: *const u8,
    stock_size: usize,
    layout: &mut Raw,
) -> *const c_char {
    let Some(device) = (unsafe { ID3D11Device::from_raw_borrowed(&device) }) else {
        return c"fixture device missing".as_ptr();
    };
    let signature = unsafe { borrow_span(signature, count as usize) };
    let elements: Vec<_> = signature
        .iter()
        .zip(0u32..)
        .map(|(input, i)| D3D11_INPUT_ELEMENT_DESC {
            SemanticName: input.SemanticName,
            SemanticIndex: input.SemanticIndex,
            Format: match input.ComponentType {
                D3D_REGISTER_COMPONENT_FLOAT32 => DXGI_FORMAT_R32G32B32A32_FLOAT,
                D3D_REGISTER_COMPONENT_UINT32 => DXGI_FORMAT_R32G32B32A32_UINT,
                _ => DXGI_FORMAT_R32G32B32A32_SINT,
            },
            InputSlot: 0,
            AlignedByteOffset: i * 16,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        })
        .collect();
    let stock = unsafe { borrow_span(stock, stock_size) };
    let mut created = None;
    let status = unsafe { device.CreateInputLayout(&elements, stock, Some(&mut created)) };
    let Some(created) = created.filter(|_| status.is_ok()) else {
        return c"layout failed".as_ptr();
    };
    *layout = created.into_raw();
    null()
}
