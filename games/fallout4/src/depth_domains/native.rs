// SPDX-License-Identifier: MIT
//! D3D11 facts sampled synchronously on the verified native render owner.

use super::{ContextFacts, ScissorFacts, TargetFacts, TextureFacts, ViewFacts};
use crate::d3d11::{identity, viewport};
use std::ffi::c_void;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_DEVICE_CONTEXT_IMMEDIATE, D3D11_RASTERIZER_DESC,
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_TEXTURE2D_DESC,
    ID3D11DeviceContext, ID3D11Resource, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::core::Interface;

fn describe(texture: &ID3D11Texture2D, output: &mut TextureFacts) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: the typed interface is live for this read.
    unsafe { texture.GetDesc(&mut desc) };
    output.texture = 1;
    output.width = desc.Width;
    output.height = desc.Height;
    output.format = desc.Format.0 as u32;
    output.mip_levels = desc.MipLevels;
    output.array_size = desc.ArraySize;
    output.sample_count = desc.SampleDesc.Count;
    output.sample_quality = desc.SampleDesc.Quality;
    output.device = unsafe { texture.GetDevice() }.as_ref().map_or(0, identity);
}

fn resource(resource: &ID3D11Resource) -> TextureFacts {
    let mut facts = TextureFacts {
        resource: identity(resource),
        ..TextureFacts::default()
    };
    if let Ok(texture) = resource.cast::<ID3D11Texture2D>() {
        describe(&texture, &mut facts);
    }
    facts
}

pub(super) fn context(key: u64) -> ContextFacts {
    let mut facts = ContextFacts::default();
    let raw = key as *mut c_void;
    // SAFETY: the sampled renderer and TLS agree on the immediate context pointer.
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return facts;
    };
    facts.immediate = u32::from(unsafe { context.GetType() } == D3D11_DEVICE_CONTEXT_IMMEDIATE);
    facts.device = unsafe { context.GetDevice() }.as_ref().map_or(0, identity);
    (facts.viewport_count, facts.viewport_copied, facts.viewport) = viewport(context);
    facts
}

pub(super) fn targets(key: u64) -> TargetFacts {
    let mut facts = TargetFacts::default();
    let raw = key as *mut c_void;
    // SAFETY: the caller holds the verified native draw context. The getter retains both views.
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return facts;
    };
    let mut rtvs = [None];
    let mut dsv = None;
    unsafe { context.OMGetRenderTargets(Some(&mut rtvs), Some(&mut dsv)) };
    if let Some(dsv) = dsv {
        facts.dsv = dsv.as_raw() as u64;
        facts.depth = unsafe { dsv.GetResource() }
            .as_ref()
            .map(resource)
            .unwrap_or_default();
        let mut desc = D3D11_DEPTH_STENCIL_VIEW_DESC::default();
        unsafe { dsv.GetDesc(&mut desc) };
        facts.dsv_format = desc.Format.0 as u32;
        facts.dsv_dimension = desc.ViewDimension.0 as u32;
        facts.dsv_mip = unsafe { desc.Anonymous.Texture2D.MipSlice };
    }
    if let [Some(rtv)] = rtvs {
        facts.rtv = rtv.as_raw() as u64;
        facts.rtv_identity = identity(&rtv);
        facts.colour = unsafe { rtv.GetResource() }
            .as_ref()
            .map(resource)
            .unwrap_or_default();
        let mut desc = D3D11_RENDER_TARGET_VIEW_DESC::default();
        unsafe { rtv.GetDesc(&mut desc) };
        facts.rtv_dimension = desc.ViewDimension.0 as u32;
        facts.rtv_mip = unsafe { desc.Anonymous.Texture2D.MipSlice };
    }
    facts
}

pub(super) fn scissor(key: u64) -> ScissorFacts {
    let mut facts = ScissorFacts::default();
    let raw = key as *mut c_void;
    // SAFETY: the caller holds the verified native draw context.
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return facts;
    };
    if let Ok(state) = unsafe { context.RSGetState() } {
        let mut desc = D3D11_RASTERIZER_DESC::default();
        unsafe { state.GetDesc(&mut desc) };
        facts.enabled = u32::from(desc.ScissorEnable.as_bool());
    }
    if facts.enabled == 0 {
        return facts;
    }
    unsafe { context.RSGetScissorRects(&mut facts.count, None) };
    if facts.count == 1 {
        let mut rect = RECT::default();
        facts.copied = 1;
        unsafe { context.RSGetScissorRects(&mut facts.copied, Some(&mut rect)) };
        facts.rect = [rect.left, rect.top, rect.right, rect.bottom];
    }
    facts
}

pub(super) fn texture(key: u64) -> TextureFacts {
    let raw = key as *mut c_void;
    // SAFETY: a pass with the matching depth lease supplied this Texture2D pointer.
    let Some(texture) = (unsafe { ID3D11Texture2D::from_raw_borrowed(&raw) }) else {
        return TextureFacts::default();
    };
    let mut facts = TextureFacts {
        resource: identity(texture),
        ..TextureFacts::default()
    };
    describe(texture, &mut facts);
    facts
}

pub(super) fn view(key: u64) -> ViewFacts {
    let raw = key as *mut c_void;
    // SAFETY: the sampled native depth record supplied its live SRV.
    let Some(view) = (unsafe { ID3D11ShaderResourceView::from_raw_borrowed(&raw) }) else {
        return ViewFacts::default();
    };
    let mut desc = D3D11_SHADER_RESOURCE_VIEW_DESC::default();
    unsafe { view.GetDesc(&mut desc) };
    let texture = unsafe { desc.Anonymous.Texture2D };
    ViewFacts {
        resource: unsafe { view.GetResource() }.as_ref().map_or(0, identity),
        device: unsafe { view.GetDevice() }.as_ref().map_or(0, identity),
        format: desc.Format.0 as u32,
        dimension: desc.ViewDimension.0 as u32,
        most_detailed_mip: texture.MostDetailedMip,
        mip_levels: texture.MipLevels,
    }
}
