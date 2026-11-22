// SPDX-License-Identifier: MIT
use super::{Constants, Handle, Pipeline, Setup, Surfaces};
use crate::com_ref::ComRef;
use crate::d3d11_output::{same_device, texture_desc};
use crate::d3d11_output_abi::rsf_d3d11_output_is_immediate;
use crate::graphics_d3d11::{
    constant_buffer, depth_state, pixel_shader, rasterizer, shader_with_log, source_view,
    vertex_shader,
};
use rsf_shader_compiler::Request;
use std::ptr;
use windows::Win32::Graphics::Direct3D::Fxc::D3DCOMPILE_ENABLE_STRICTNESS;
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_D24_UNORM_S8_UINT, DXGI_FORMAT_R24_UNORM_X8_TYPELESS, DXGI_FORMAT_R24G8_TYPELESS,
    DXGI_FORMAT_X24_TYPELESS_G8_UINT,
};
use windows::core::Interface;

const SHADER: &[u8] = include_bytes!("../../../graphics/shaders/depth_scale.hlsl");

pub(super) unsafe fn immediate_device(context: Handle) -> Option<ID3D11Device> {
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return None;
    }
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }?;
    unsafe { context.GetDevice() }.ok()
}

pub(super) fn make_pipeline(device: &ID3D11Device, setup: &Setup) -> Option<[ComRef; 14]> {
    if unsafe { device.GetFeatureLevel() }.0 < D3D_FEATURE_LEVEL_11_0.0 {
        return None;
    }
    let compile = |entry, target| {
        shader_with_log(
            Request {
                source: SHADER,
                name: c"depth_scale",
                entry,
                target,
                flags: D3DCOMPILE_ENABLE_STRICTNESS,
            },
            setup.log,
            setup.log_user,
        )
    };
    let vs = vertex_shader(device, &compile(c"vertex_main", c"vs_5_0")?)?;
    let depth_shader = pixel_shader(device, &compile(c"depth_main", c"ps_5_0")?)?;
    let stencil_shader = pixel_shader(device, &compile(c"stencil_main", c"ps_5_0")?)?;
    let face = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_REPLACE,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    };
    let mut desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        StencilEnable: true.into(),
        StencilReadMask: 0xff,
        StencilWriteMask: 0xff,
        FrontFace: face,
        BackFace: face,
    };
    let depth = depth_state(device, &desc)?;
    desc.DepthEnable = false.into();
    desc.DepthWriteMask = D3D11_DEPTH_WRITE_MASK_ZERO;
    let mut stencil = [const { None }; 8];
    for (bit, state) in stencil.iter_mut().enumerate() {
        desc.StencilWriteMask = 1u8 << bit;
        *state = Some(depth_state(device, &desc)?);
    }
    let raster = rasterizer(device)?;
    let buffer = constant_buffer(device, size_of::<Constants>() as u32, false)?;
    let [s0, s1, s2, s3, s4, s5, s6, s7] =
        stencil.map(|state| state.expect("all stencil states created").into_raw());
    unsafe {
        ComRef::adopt_all([
            vs.into_raw(),
            depth_shader.into_raw(),
            stencil_shader.into_raw(),
            depth.into_raw(),
            s0,
            s1,
            s2,
            s3,
            s4,
            s5,
            s6,
            s7,
            raster.into_raw(),
            buffer.into_raw(),
        ])
    }
}

pub(super) unsafe fn describe(device: &ID3D11Device, texture: Handle) -> Option<[u32; 2]> {
    let desc = unsafe { texture_desc(texture) };
    if !unsafe { same_device(device.as_raw(), texture) }
        || desc.format != DXGI_FORMAT_R24G8_TYPELESS.0 as u32
        || desc.mips != 1
        || desc.layers != 1
        || desc.samples != 1
        || desc.usage != D3D11_USAGE_DEFAULT.0 as u32
        || desc.bind_flags & D3D11_BIND_DEPTH_STENCIL.0 as u32 == 0
    {
        return None;
    }
    Some([desc.width, desc.height])
}

pub(super) unsafe fn make_surfaces(device: &ID3D11Device, source: Handle) -> Option<[ComRef; 5]> {
    let source = unsafe { ID3D11Texture2D::from_raw_borrowed(&source) }?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source.GetDesc(&mut desc) };
    desc.BindFlags = (D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_DEPTH_STENCIL).0 as u32;
    desc.CPUAccessFlags = 0;
    desc.MiscFlags = 0;
    let make_texture = || {
        let mut texture = None;
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
        texture
    };
    let snapshot = make_texture()?;
    let output = make_texture()?;
    let read_plane = |format| unsafe {
        ComRef::from_owned(source_view(device.as_raw(), snapshot.as_raw(), format))
    };
    let depth = read_plane(DXGI_FORMAT_R24_UNORM_X8_TYPELESS.0 as u32)?;
    let stencil = read_plane(DXGI_FORMAT_X24_TYPELESS_G8_UINT.0 as u32)?;
    let dsv = D3D11_DEPTH_STENCIL_VIEW_DESC {
        Format: DXGI_FORMAT_D24_UNORM_S8_UINT,
        ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let mut target = None;
    unsafe { device.CreateDepthStencilView(&output, Some(&dsv), Some(&mut target)) }.ok()?;
    Some([
        unsafe { ComRef::from_owned(snapshot.into_raw()) }?,
        unsafe { ComRef::from_owned(output.into_raw()) }?,
        depth,
        stencil,
        unsafe { ComRef::from_owned(target?.into_raw()) }?,
    ])
}

pub(super) fn bind(context: &ID3D11DeviceContext, p: &Pipeline, s: &Surfaces, rect: &[i32; 4]) {
    let raw = context.as_raw();
    let table = context.vtable();
    let viewport = D3D11_VIEWPORT {
        TopLeftX: rect[0] as f32,
        TopLeftY: rect[1] as f32,
        Width: (rect[2] - rect[0]) as f32,
        Height: (rect[3] - rect[1]) as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    unsafe {
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        (table.VSSetShader)(raw, p[0], ptr::null(), 0);
        context.RSSetState(ID3D11RasterizerState::from_raw_borrowed(&p[12]));
        context.RSSetViewports(Some(&[viewport]));
        context.OMSetRenderTargets(None, ID3D11DepthStencilView::from_raw_borrowed(&s[4]));
        (table.PSSetShaderResources)(raw, 0, 2, s[2..4].as_ptr());
        (table.PSSetConstantBuffers)(raw, 0, 1, &p[13]);
    }
}

pub(super) fn draw(
    context: &ID3D11DeviceContext,
    p: &Pipeline,
    constants: &Constants,
    step: usize,
) {
    // The depth draw zeroes stencil only inside the destination viewport.
    let (shader, reference) = if step == 0 { (p[1], 0) } else { (p[2], 0xff) };
    let buffer = unsafe { ID3D11Buffer::from_raw_borrowed(&p[13]) }.expect("live constants");
    unsafe {
        (context.vtable().PSSetShader)(context.as_raw(), shader, ptr::null(), 0);
        context.UpdateSubresource(buffer, 0, None, ptr::from_ref(constants).cast(), 0, 0);
        context.OMSetDepthStencilState(
            ID3D11DepthStencilState::from_raw_borrowed(&p[3 + step]),
            reference,
        );
        context.Draw(3, 0);
    }
}
