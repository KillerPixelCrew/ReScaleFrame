// SPDX-License-Identifier: MIT
use super::{Handle, PipelineInfo, Resources, TextureInfo, ViewInfo};
use crate::com_ref::ComRef;
use crate::frame_tap_abi::rsf_frame_tap_geometry;
use crate::graphics_d3d11::depth_state;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_D32_FLOAT_S8X24_UINT, DXGI_FORMAT_R32G8X24_TYPELESS, DXGI_SAMPLE_DESC,
};
use windows::core::Interface;

pub(super) unsafe fn make(device: Handle, width: u32, height: u32) -> Option<[ComRef; 3]> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R32G8X24_TYPELESS,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_DEPTH_STENCIL | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        ..Default::default()
    };
    let view_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
        Format: DXGI_FORMAT_D32_FLOAT_S8X24_UINT,
        ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    let state_desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: true.into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ALL,
        DepthFunc: D3D11_COMPARISON_GREATER_EQUAL,
        ..Default::default()
    };
    let mut texture = None;
    let mut view = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.ok()?;
    let texture = texture?;
    unsafe { device.CreateDepthStencilView(&texture, Some(&view_desc), Some(&mut view)) }.ok()?;
    let view = view?;
    let state = depth_state(device, &state_desc)?;
    unsafe { ComRef::adopt_all([texture.into_raw(), view.into_raw(), state.into_raw()]) }
}

pub(super) fn view_info(view: &ID3D11DepthStencilView) -> ViewInfo {
    let mut desc = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut desc) };
    ViewInfo {
        format: desc.Format.0 as u32,
        dimension: desc.ViewDimension.0 as u32,
        flags: desc.Flags,
        mip_slice: unsafe { desc.Anonymous.Texture2D.MipSlice },
    }
}

pub(super) fn view_texture(view: &ID3D11DepthStencilView) -> Option<(ComRef, TextureInfo)> {
    let texture = unsafe { view.GetResource() }
        .ok()?
        .cast::<ID3D11Texture2D>()
        .ok()?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    let info = TextureInfo {
        width: desc.Width,
        height: desc.Height,
        format: desc.Format.0 as u32,
        samples: desc.SampleDesc.Count,
        quality: desc.SampleDesc.Quality,
        mips: desc.MipLevels,
        array: desc.ArraySize,
    };
    Some((unsafe { ComRef::from_owned(texture.into_raw()) }?, info))
}

pub(super) fn pipeline(context: &ID3D11DeviceContext) -> PipelineInfo {
    let mut stages = (None, None, None);
    let mut stream_out = [const { None }; D3D11_SO_BUFFER_SLOT_COUNT as usize];
    let mut uavs = [const { None }; D3D11_PS_CS_UAV_REGISTER_COUNT as usize];
    let mut state = None;
    let mut reference = 0;
    let mut viewport = D3D11_VIEWPORT::default();
    let mut count = 1;
    unsafe {
        context.HSGetShader(&mut stages.0, None, None);
        context.DSGetShader(&mut stages.1, None, None);
        context.GSGetShader(&mut stages.2, None, None);
        context.SOGetTargets(Some(&mut stream_out));
        context.OMGetRenderTargetsAndUnorderedAccessViews(None, None, 0, Some(&mut uavs));
        context.OMGetDepthStencilState(Some(&mut state), Some(&mut reference));
    }
    let mut pipeline = PipelineInfo {
        stages: u32::from(stages.0.is_some() || stages.1.is_some() || stages.2.is_some()),
        stream_out: u32::from(stream_out.iter().any(Option::is_some)),
        uavs: u32::from(uavs.iter().any(Option::is_some)),
        ..Default::default()
    };
    if let Some(state) = state {
        let mut desc = D3D11_DEPTH_STENCIL_DESC::default();
        unsafe { state.GetDesc(&mut desc) };
        pipeline.depth_state = 1;
        pipeline.depth_enable = u32::from(desc.DepthEnable.as_bool());
        pipeline.depth_write_mask = desc.DepthWriteMask.0 as u32;
        pipeline.depth_func = desc.DepthFunc.0 as u32;
        pipeline.stencil_enable = u32::from(desc.StencilEnable.as_bool());
        pipeline.front_func = desc.FrontFace.StencilFunc.0 as u32;
        pipeline.back_func = desc.BackFace.StencilFunc.0 as u32;
    }
    unsafe { context.RSGetViewports(&mut count, Some(&mut viewport)) };
    pipeline.viewport_count = count;
    pipeline.viewport = [
        viewport.TopLeftX,
        viewport.TopLeftY,
        viewport.Width,
        viewport.Height,
        viewport.MinDepth,
        viewport.MaxDepth,
    ];
    pipeline
}

pub(super) fn draw(
    context: &ID3D11DeviceContext,
    resources: &Resources,
    geometry: &rsf_frame_tap_geometry,
) {
    unsafe {
        context.OMSetRenderTargets(
            None,
            ID3D11DepthStencilView::from_raw_borrowed(&resources[1]),
        );
        context
            .OMSetDepthStencilState(ID3D11DepthStencilState::from_raw_borrowed(&resources[2]), 0);
        context.PSSetShader(None, None);
        match geometry.kind {
            0 => context.Draw(geometry.count, geometry.start),
            1 => context.DrawIndexed(geometry.count, geometry.start, geometry.base_vertex),
            2 => context.DrawInstanced(
                geometry.count,
                geometry.instances,
                geometry.start,
                geometry.start_instance,
            ),
            3 => context.DrawIndexedInstanced(
                geometry.count,
                geometry.instances,
                geometry.start,
                geometry.base_vertex,
                geometry.start_instance,
            ),
            _ => {}
        }
    }
}
