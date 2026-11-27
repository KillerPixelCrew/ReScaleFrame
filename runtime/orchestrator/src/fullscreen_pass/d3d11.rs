// SPDX-License-Identifier: MIT
use super::{Constants, Handle, RESOURCE_FAILED, SHADER_FAILED, Setup};
use crate::d3d11_output::SavedBindings;
use crate::graphics_d3d11::{
    constant_buffer, depth_state, pixel_shader, rasterizer, rsf_call_native_log,
    rsf_d3d11_state_restore, shader_with_log, vertex_shader,
};
use rsf_graphics::d3d11_state::rsf_d3d11_state_save_independent;
use rsf_shader_compiler::Request;
use std::ptr;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::IUnknown;
use windows::core::Interface;

const SHADER: &[u8] = include_bytes!("../../../graphics/shaders/fullscreen_pass.hlsl");

fn sampler(device: &ID3D11Device, filter: D3D11_FILTER) -> Option<ID3D11SamplerState> {
    let desc = D3D11_SAMPLER_DESC {
        Filter: filter,
        AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        MaxLOD: D3D11_FLOAT32_MAX,
        ..Default::default()
    };
    let mut sampler = None;
    unsafe { device.CreateSamplerState(&desc, Some(&mut sampler)) }.ok()?;
    sampler
}

fn blend(device: &ID3D11Device, premultiplied: bool) -> Option<ID3D11BlendState> {
    let mut desc = D3D11_BLEND_DESC::default();
    let target = &mut desc.RenderTarget[0];
    target.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
    if premultiplied {
        target.BlendEnable = true.into();
        target.SrcBlend = D3D11_BLEND_ONE;
        target.DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOp = D3D11_BLEND_OP_ADD;
        target.SrcBlendAlpha = D3D11_BLEND_ONE;
        target.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
        target.BlendOpAlpha = D3D11_BLEND_OP_ADD;
    }
    let mut blend = None;
    unsafe { device.CreateBlendState(&desc, Some(&mut blend)) }.ok()?;
    blend
}

pub(super) unsafe fn make(device: Handle, setup: &Setup) -> Result<[IUnknown; 9], i32> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }.expect("live device");
    let compile = |entry, target| {
        shader_with_log(
            Request {
                source: SHADER,
                name: c"fullscreen_pass",
                entry,
                target,
                flags: 0,
            },
            setup.log,
            setup.log_user,
        )
    };
    let shaders = || {
        Some((
            vertex_shader(device, &compile(c"vertex_main", c"vs_5_0")?)?,
            pixel_shader(device, &compile(c"pixel_main", c"ps_5_0")?)?,
        ))
    };
    let Some((vs, ps)) = shaders() else {
        unsafe {
            rsf_call_native_log(
                setup.log,
                setup.log_user,
                c"the fullscreen pass shaders could not be created".as_ptr(),
            )
        };
        return Err(SHADER_FAILED);
    };
    let make = || {
        let point = sampler(device, D3D11_FILTER_MIN_MAG_MIP_POINT)?;
        let linear = sampler(device, D3D11_FILTER_MIN_MAG_MIP_LINEAR)?;
        let opaque = blend(device, false)?;
        let premultiplied = blend(device, true)?;
        let face = D3D11_DEPTH_STENCILOP_DESC {
            StencilFailOp: D3D11_STENCIL_OP_KEEP,
            StencilDepthFailOp: D3D11_STENCIL_OP_KEEP,
            StencilPassOp: D3D11_STENCIL_OP_KEEP,
            StencilFunc: D3D11_COMPARISON_ALWAYS,
        };
        let depth = depth_state(
            device,
            &D3D11_DEPTH_STENCIL_DESC {
                DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                DepthFunc: D3D11_COMPARISON_ALWAYS,
                StencilReadMask: D3D11_DEFAULT_STENCIL_READ_MASK as u8,
                StencilWriteMask: D3D11_DEFAULT_STENCIL_WRITE_MASK as u8,
                FrontFace: face,
                BackFace: face,
                ..Default::default()
            },
        )?;
        let raster = rasterizer(device)?;
        let constants = constant_buffer(device, size_of::<Constants>() as u32, true)?;
        Some([
            vs.into(),
            ps.into(),
            point.into(),
            linear.into(),
            opaque.into(),
            premultiplied.into(),
            depth.into(),
            raster.into(),
            constants.into(),
        ])
    };
    make().ok_or_else(|| {
        unsafe {
            rsf_call_native_log(
                setup.log,
                setup.log_user,
                c"the fullscreen pass pipeline state could not be created".as_ptr(),
            )
        };
        RESOURCE_FAILED
    })
}

pub(super) unsafe fn target_extent(target: Handle) -> Option<[u32; 2]> {
    let target = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&target) }?;
    let texture = unsafe { target.GetResource() }
        .ok()?
        .cast::<ID3D11Texture2D>()
        .ok()?;
    let mut view = D3D11_RENDER_TARGET_VIEW_DESC::default();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        target.GetDesc(&mut view);
        texture.GetDesc(&mut desc);
    }
    if desc.Width == 0 || desc.Height == 0 {
        return None;
    }
    let mip = match view.ViewDimension {
        D3D11_RTV_DIMENSION_TEXTURE2D => unsafe { view.Anonymous.Texture2D.MipSlice },
        D3D11_RTV_DIMENSION_TEXTURE2DARRAY => unsafe { view.Anonymous.Texture2DArray.MipSlice },
        _ => 0,
    };
    Some([
        desc.Width.checked_shr(mip)?.max(1),
        desc.Height.checked_shr(mip)?.max(1),
    ])
}

#[allow(clippy::too_many_arguments)]
pub(super) unsafe fn draw(
    raw: Handle,
    target: Handle,
    source: Handle,
    resources: &[IUnknown; 9],
    viewport: [u32; 2],
    linear: bool,
    blend: bool,
    constants: &Constants,
) -> Option<()> {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }?;
    let [
        vs,
        ps,
        point,
        linear_sampler,
        opaque,
        premultiplied,
        depth,
        raster,
        buffer,
    ] = resources.each_ref().map(Interface::as_raw);
    let cb = unsafe { ID3D11Buffer::from_raw_borrowed(&buffer) }?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context
            .Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
            .ok()?;
        if mapped.pData.is_null() {
            context.Unmap(cb, 0);
            return None;
        }
        ptr::copy_nonoverlapping(ptr::from_ref(constants), mapped.pData.cast(), 1);
        context.Unmap(cb, 0);
    }
    // An independent snapshot restores every implicit alias unbind before the next pass.
    let _saved = unsafe {
        SavedBindings::new(
            raw,
            rsf_d3d11_state_save_independent,
            rsf_d3d11_state_restore,
        )
    }?;
    let table = context.vtable();
    let sampler = if linear { linear_sampler } else { point };
    let blend = if blend { premultiplied } else { opaque };
    let viewport = D3D11_VIEWPORT {
        Width: viewport[0] as f32,
        Height: viewport[1] as f32,
        MaxDepth: 1.0,
        ..Default::default()
    };
    unsafe {
        context.SetPredication(None, false);
        context.SOSetTargets(0, None, None);
        (table.OMSetRenderTargets)(raw, 1, &target, ptr::null_mut());
        (table.VSSetShader)(raw, vs, ptr::null(), 0);
        context.HSSetShader(None, None);
        context.DSSetShader(None, None);
        context.GSSetShader(None, None);
        (table.PSSetShader)(raw, ps, ptr::null(), 0);
        context.IASetInputLayout(None);
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        (table.PSSetShaderResources)(raw, 0, 1, &source);
        (table.PSSetSamplers)(raw, 0, 1, &sampler);
        (table.PSSetConstantBuffers)(raw, 0, 1, &buffer);
        context.OMSetBlendState(
            ID3D11BlendState::from_raw_borrowed(&blend),
            Some(&[0.0; 4]),
            u32::MAX,
        );
        context.OMSetDepthStencilState(ID3D11DepthStencilState::from_raw_borrowed(&depth), 0);
        context.RSSetState(ID3D11RasterizerState::from_raw_borrowed(&raster));
        context.RSSetViewports(Some(&[viewport]));
        context.Draw(3, 0);
    }
    Some(())
}
