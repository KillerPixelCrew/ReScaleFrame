// SPDX-License-Identifier: MIT
use super::{Handle, IUnknown, RESOURCE_FAILED, SHADER_FAILED, Setup, Vertex};
use crate::com_ref;
use rsf_shader_compiler::{Compiler, Request, compile};
use std::{ffi::CStr, mem::offset_of, ptr::null_mut};
use windows::{
    Win32::Graphics::{
        Direct3D11::*,
        Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_FORMAT_R32G32_FLOAT},
    },
    core::{BOOL, Interface, PCSTR},
};

const SHADER: &[u8] = include_bytes!("overlay.hlsl");
const _: () = {
    assert!(size_of::<Vertex>() == 20);
    assert!(offset_of!(Vertex, u) == 8);
    assert!(offset_of!(Vertex, color) == 16);
};

fn shader_code(setup: &Setup, entry: &CStr, target: &CStr) -> Option<Vec<u8>> {
    let Some(compiler) = Compiler::system("d3dcompiler_47.dll") else {
        setup.say("shader overlay_renderer: d3dcompiler_47 is not available");
        return None;
    };
    let compiler_entry = compiler.entry()?;
    // Shader model 4_0 supports feature-level 10 hardware. Compile the original source
    // at startup so the native and cross builds use one path.
    let output = unsafe {
        compile(
            compiler_entry,
            Request {
                source: SHADER,
                name: c"overlay_renderer",
                entry,
                target,
                flags: 0,
            },
            &[],
            null_mut(),
        )
    };
    if output.status >= 0
        && let Some(code) = output.code
    {
        return Some(code);
    }
    let diagnostic = output.errors.as_deref().unwrap_or_default();
    let end = diagnostic
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(diagnostic.len());
    setup.say(&format!(
        "shader overlay_renderer ({}): compile failed 0x{:08x} {}",
        entry.to_string_lossy(),
        output.status as u32,
        String::from_utf8_lossy(&diagnostic[..end])
    ));
    None
}

fn resource<T: Interface>(
    setup: &Setup,
    diagnostic: &str,
    create: impl FnOnce(&mut Option<T>) -> windows::core::Result<()>,
) -> Result<IUnknown, i32> {
    let mut object = None;
    if create(&mut object).is_ok()
        && let Some(object) = object
        && let Some(reference) = unsafe { com_ref::owned(object.into_raw()) }
    {
        return Ok(reference);
    }
    setup.say(diagnostic);
    Err(RESOURCE_FAILED)
}

pub(super) unsafe fn create(device: Handle, setup: &Setup) -> Result<[IUnknown; 8], i32> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.ok_or(RESOURCE_FAILED)?;
    let vertex_code = shader_code(setup, c"vertex_main", c"vs_4_0").ok_or(SHADER_FAILED)?;
    let pixel_code = shader_code(setup, c"pixel_main", c"ps_4_0").ok_or(SHADER_FAILED)?;
    let linkage = None::<&ID3D11ClassLinkage>;
    let mut vertex = None;
    let mut pixel = None;
    let vertex_result =
        unsafe { device.CreateVertexShader(&vertex_code, linkage, Some(&mut vertex)) };
    let pixel_result = unsafe { device.CreatePixelShader(&pixel_code, linkage, Some(&mut pixel)) };
    let (Ok(()), Ok(()), Some(vertex), Some(pixel)) = (vertex_result, pixel_result, vertex, pixel)
    else {
        setup.say("overlay shaders could not be created");
        return Err(SHADER_FAILED);
    };
    let vertex = unsafe { com_ref::owned(vertex.into_raw()) }.ok_or(SHADER_FAILED)?;
    let pixel = unsafe { com_ref::owned(pixel.into_raw()) }.ok_or(SHADER_FAILED)?;

    let elements = [
        (c"POSITION", DXGI_FORMAT_R32G32_FLOAT, 0),
        (
            c"TEXCOORD",
            DXGI_FORMAT_R32G32_FLOAT,
            offset_of!(Vertex, u) as u32,
        ),
        (
            c"COLOR",
            DXGI_FORMAT_R8G8B8A8_UNORM,
            offset_of!(Vertex, color) as u32,
        ),
    ]
    .map(|(name, format, offset)| D3D11_INPUT_ELEMENT_DESC {
        SemanticName: PCSTR(name.as_ptr().cast()),
        Format: format,
        AlignedByteOffset: offset,
        InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
        ..Default::default()
    });
    let layout = resource(
        setup,
        "overlay input layout could not be created",
        |out| unsafe { device.CreateInputLayout(&elements, &vertex_code, Some(out)) },
    )?;

    let constants_desc = D3D11_BUFFER_DESC {
        ByteWidth: size_of::<[f32; 4]>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let constants = resource(
        setup,
        "overlay constant buffer could not be created",
        |out| unsafe { device.CreateBuffer(&constants_desc, None, Some(out)) },
    )?;

    let mut blend_desc = D3D11_BLEND_DESC::default();
    // The atlas and vertex alpha are both premultiplied.
    blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: BOOL(1),
        SrcBlend: D3D11_BLEND_ONE,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let blend = resource(
        setup,
        "overlay blend state could not be created",
        |out| unsafe { device.CreateBlendState(&blend_desc, Some(out)) },
    )?;

    // No depth or stencil access. Populate the unused stencil enums with valid values.
    let stencil = D3D11_DEPTH_STENCILOP_DESC {
        StencilFailOp: D3D11_STENCIL_OP_KEEP,
        StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
        StencilPassOp: D3D11_STENCIL_OP_KEEP,
        StencilFunc: D3D11_COMPARISON_ALWAYS,
    };
    let depth_desc = D3D11_DEPTH_STENCIL_DESC {
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: D3D11_COMPARISON_ALWAYS,
        StencilReadMask: D3D11_DEFAULT_STENCIL_READ_MASK as u8,
        StencilWriteMask: D3D11_DEFAULT_STENCIL_WRITE_MASK as u8,
        FrontFace: stencil,
        BackFace: stencil,
        ..Default::default()
    };
    let depth = resource(
        setup,
        "overlay depth stencil state could not be created",
        |out| unsafe { device.CreateDepthStencilState(&depth_desc, Some(out)) },
    )?;

    let raster_desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        DepthClipEnable: BOOL(1),
        ScissorEnable: BOOL(1),
        ..Default::default()
    };
    let raster = resource(
        setup,
        "overlay rasterizer state could not be created",
        |out| unsafe { device.CreateRasterizerState(&raster_desc, Some(out)) },
    )?;

    let sampler_desc = D3D11_SAMPLER_DESC {
        Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        ComparisonFunc: D3D11_COMPARISON_ALWAYS,
        MaxLOD: D3D11_FLOAT32_MAX,
        ..Default::default()
    };
    let sampler = resource(
        setup,
        "overlay sampler could not be created",
        |out| unsafe { device.CreateSamplerState(&sampler_desc, Some(out)) },
    )?;
    Ok([
        vertex, pixel, layout, constants, blend, depth, raster, sampler,
    ])
}
