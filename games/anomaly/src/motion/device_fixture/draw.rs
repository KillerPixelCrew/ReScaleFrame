// SPDX-License-Identifier: MIT

mod bindings;
mod resources;

use crate::sr_fixture_readback::rsf_anomaly_sr_fixture_readback;

pub(super) use bindings::Bindings;
pub(super) use bindings::{
    rsf_anomaly_motion_fixture_bind_core, rsf_anomaly_motion_fixture_bind_stage,
    rsf_anomaly_motion_fixture_bindings_drop, rsf_anomaly_motion_fixture_bindings_new,
    rsf_anomaly_motion_fixture_patch_core,
};
pub(super) use resources::{
    rsf_anomaly_motion_fixture_make_layout, rsf_anomaly_motion_fixture_make_targets,
};
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::Interface;

type Raw = *mut c_void;

/// Borrowed input-assembler and shader state for one draw.
pub(super) struct Input {
    pub(super) layout: Raw,
    pub(super) vertex_buffer: Raw,
    pub(super) vs: Raw,
    pub(super) hs: Raw,
    pub(super) ds: Raw,
    pub(super) ps: Raw,
    pub(super) stride: u32,
    pub(super) tessellated: bool,
}

/// Borrowed output-merger state for one draw.
pub(super) struct Output {
    pub(super) position: Raw,
    pub(super) albedo: Raw,
    pub(super) motion: Raw,
    pub(super) depth: Raw,
    pub(super) depth_state: Raw,
    pub(super) raster: Raw,
    pub(super) size: u32,
}

/// One named constant replaced while patching a shader's constant buffers.
pub(super) struct Override {
    pub(super) name: *const c_char,
    pub(super) values: *const f32,
    pub(super) count: usize,
}

/// The eight COM references `rsf_anomaly_motion_fixture_make_targets` transfers together.
pub(super) struct Targets {
    pub(super) position_texture: Raw,
    pub(super) position_rtv: Raw,
    pub(super) albedo_texture: Raw,
    pub(super) albedo_rtv: Raw,
    pub(super) depth_texture: Raw,
    pub(super) depth_view: Raw,
    pub(super) depth_state: Raw,
    pub(super) raster: Raw,
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_read(
    device: Raw,
    context: Raw,
    texture: Raw,
    out: Raw,
    bytes: usize,
) -> bool {
    unsafe {
        rsf_anomaly_sr_fixture_readback(device, context, texture, null_mut(), out.cast(), bytes)
    }
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_clear(
    raw: Raw,
    targets: &Output,
    stencil: bool,
) {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return;
    };
    let (Some(position), Some(albedo), Some(depth)) = (unsafe {
        (
            ID3D11RenderTargetView::from_raw_borrowed(&targets.position),
            ID3D11RenderTargetView::from_raw_borrowed(&targets.albedo),
            ID3D11DepthStencilView::from_raw_borrowed(&targets.depth),
        )
    }) else {
        return;
    };
    let flags = if stencil {
        D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL
    } else {
        D3D11_CLEAR_DEPTH
    };
    unsafe {
        context.ClearRenderTargetView(position, &[-999.; 4]);
        context.ClearRenderTargetView(albedo, &[0.; 4]);
        context.ClearDepthStencilView(depth, flags.0, 1., 0);
        if let Some(motion) = ID3D11RenderTargetView::from_raw_borrowed(&targets.motion) {
            context.ClearRenderTargetView(motion, &[0.; 4]);
        }
    }
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_input_bind(raw: Raw, input: &Input) {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return;
    };
    let sdk = context.vtable();
    let topology = if input.tessellated {
        D3D_PRIMITIVE_TOPOLOGY_3_CONTROL_POINT_PATCHLIST
    } else {
        D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
    };
    unsafe {
        (sdk.IASetInputLayout)(raw, input.layout);
        context.IASetPrimitiveTopology(topology);
        if !input.vertex_buffer.is_null() {
            (sdk.IASetVertexBuffers)(raw, 0, 1, &input.vertex_buffer, &input.stride, &0);
        }
        (sdk.VSSetShader)(raw, input.vs, null(), 0);
        if input.tessellated {
            (sdk.HSSetShader)(raw, input.hs, null(), 0);
            (sdk.DSSetShader)(raw, input.ds, null(), 0);
        }
        (sdk.PSSetShader)(raw, input.ps, null(), 0);
    }
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_output_bind(raw: Raw, targets: &Output) {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        return;
    };
    let sdk = context.vtable();
    let views = [targets.position, targets.albedo, targets.motion];
    let count = if targets.motion.is_null() { 2 } else { 3 };
    let viewport = D3D11_VIEWPORT {
        Width: targets.size as f32,
        Height: targets.size as f32,
        MaxDepth: 1.,
        ..Default::default()
    };
    unsafe {
        (sdk.OMSetRenderTargets)(raw, count, views.as_ptr(), targets.depth);
        (sdk.OMSetDepthStencilState)(raw, targets.depth_state, 0);
        (sdk.RSSetState)(raw, targets.raster);
        context.RSSetViewports(Some(&[viewport]));
    }
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_draw(raw: Raw, count: u32) {
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) } {
        unsafe { context.Draw(count, 0) };
    }
}

pub(super) unsafe fn rsf_anomaly_motion_fixture_draw_quad(
    raw: Raw,
    buffer: Raw,
    stride: u32,
) {
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) } {
        unsafe {
            (context.vtable().IASetVertexBuffers)(raw, 0, 1, &buffer, &stride, &0);
            context.Draw(6, 0);
        }
    }
}
