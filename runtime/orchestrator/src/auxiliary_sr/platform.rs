// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_d3d12::borrow;
use crate::d3d11_output_platform::rsf_d3d11_output_create_rtv;
use crate::graphics_d3d11::{
    constant_buffer, pixel_shader, rasterizer, rsf_d3d11_state_restore, rsf_d3d11_state_save,
    shader_with_log, vertex_shader,
};
use rsf_graphics::d3d11_state::{rsf_d3d11_copy_resource, rsf_d3d11_create_shader_view};
use rsf_shader_compiler::Request;
use windows::Win32::Graphics::{
    Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCOMPILE_ENABLE_STRICTNESS},
    Direct3D11::*,
    Dxgi::Common::*,
};
use windows::core::Interface;

type Raw = *mut c_void;
const SHADER: &[u8] = include_bytes!("scale.hlsl");

const _: () = assert!(size_of::<AuxDrawConstants>() == 64);

unsafe extern "C" fn immediate_device(context: Raw) -> Raw {
    let context = unsafe { borrow::<ID3D11DeviceContext>(&context) };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return ptr::null_mut();
    }
    unsafe { context.GetDevice() }.map_or(ptr::null_mut(), Interface::into_raw)
}
unsafe extern "C" fn device_ready(device: Raw) -> i32 {
    i32::from(unsafe { borrow::<ID3D11Device>(&device).GetDeviceRemovedReason() }.is_ok())
}
unsafe extern "C" fn describe_texture(texture: Raw, device: Raw, out: *mut AuxTextureFacts) -> i32 {
    let texture = unsafe { borrow::<ID3D11Texture2D>(&texture) };
    let owner = unsafe { texture.GetDevice() }.ok();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        texture.GetDesc(&mut desc);
        out.write(AuxTextureFacts {
            info: AuxTextureInfo {
                width: desc.Width,
                height: desc.Height,
                format: desc.Format.0 as u32,
                sample_quality: desc.SampleDesc.Quality,
                depth_stencil: u32::from(desc.BindFlags & D3D11_BIND_DEPTH_STENCIL.0 as u32 != 0),
            },
            same_device: u32::from(owner.is_some_and(|owner| owner.as_raw() == device)),
            array_size: desc.ArraySize,
            mip_levels: desc.MipLevels,
            sample_count: desc.SampleDesc.Count,
            usage: desc.Usage.0 as u32,
        });
    }
    1
}
unsafe extern "C" fn packed_colour_support(device: Raw, flags: *mut u32) -> i32 {
    let result =
        unsafe { borrow::<ID3D11Device>(&device).CheckFormatSupport(DXGI_FORMAT_R11G11B10_FLOAT) };
    let supported = result.is_ok();
    unsafe { flags.write(result.unwrap_or_default()) };
    i32::from(supported)
}
unsafe extern "C" fn create_pipeline(
    device: Raw,
    log: rsf_game_log_fn,
    user: Raw,
    out: *mut AuxColourPipeline,
) -> i32 {
    let device = unsafe { borrow::<ID3D11Device>(&device) };
    let compile = |entry, target| {
        shader_with_log(
            Request {
                source: SHADER,
                name: c"auxiliary_scale",
                entry,
                target,
                flags: D3DCOMPILE_ENABLE_STRICTNESS,
            },
            log,
            user,
        )
    };
    let build = || {
        let vertex = vertex_shader(device, &compile(c"vertex_main", c"vs_5_0")?)?;
        let pixel = pixel_shader(device, &compile(c"pixel_main", c"ps_5_0")?)?;
        let raster = rasterizer(device)?;
        let constants = constant_buffer(device, size_of::<AuxDrawConstants>() as u32, false)?;
        Some(AuxColourPipeline {
            vertex: vertex.into_raw(),
            pixel: pixel.into_raw(),
            raster: raster.into_raw(),
            constants: constants.into_raw(),
        })
    };
    let Some(pipeline) = build() else {
        return 0;
    };
    unsafe { out.write(pipeline) };
    1
}
unsafe extern "C" fn create_texture(device: Raw, info: *const AuxTextureInfo, usage: u32) -> Raw {
    let info = unsafe { &*info };
    let bind = match usage {
        AUX_TEXTURE_SOURCE => D3D11_BIND_SHADER_RESOURCE.0,
        AUX_TEXTURE_OUTPUT => D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0,
        _ => 0,
    };
    let desc = D3D11_TEXTURE2D_DESC {
        Width: info.width,
        Height: info.height,
        Format: DXGI_FORMAT(info.format as i32),
        MipLevels: 1,
        ArraySize: 1,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: info.sample_quality,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind as u32,
        ..Default::default()
    };
    let mut texture = None;
    let device = unsafe { borrow::<ID3D11Device>(&device) };
    if unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }.is_err() {
        return ptr::null_mut();
    }
    texture.map_or(ptr::null_mut(), Interface::into_raw)
}
unsafe extern "C" fn clear(context: Raw) {
    unsafe { borrow::<ID3D11DeviceContext>(&context).ClearState() };
}
unsafe extern "C" fn draw(
    raw: Raw,
    pipeline: *const AuxColourPipeline,
    view: Raw,
    target: Raw,
    params: *const AuxDrawConstants,
) {
    let context = unsafe { borrow::<ID3D11DeviceContext>(&raw) };
    let table = context.vtable();
    let pipeline = unsafe { &*pipeline };
    let [left, top, right, bottom] = unsafe { (*params).output };
    let viewport = D3D11_VIEWPORT {
        TopLeftX: left,
        TopLeftY: top,
        Width: right - left,
        Height: bottom - top,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    let constants = unsafe { borrow::<ID3D11Buffer>(&pipeline.constants) };
    unsafe {
        context.RSSetViewports(Some(&[viewport]));
        context.RSSetState(borrow::<ID3D11RasterizerState>(&pipeline.raster));
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
        (table.VSSetShader)(raw, pipeline.vertex, ptr::null(), 0);
        (table.PSSetShader)(raw, pipeline.pixel, ptr::null(), 0);
        (table.OMSetRenderTargets)(raw, 1, &target, ptr::null_mut());
        (table.PSSetShaderResources)(raw, 0, 1, &view);
        (table.PSSetConstantBuffers)(raw, 0, 1, &pipeline.constants);
        context.UpdateSubresource(constants, 0, None, params.cast(), 0, 0);
        context.Draw(3, 0);
    }
}
static PLATFORM: AuxPlatform = AuxPlatform {
    immediate_device,
    device_ready,
    describe_texture,
    packed_colour_support,
    create_pipeline,
    create_texture,
    create_source_view: rsf_d3d11_create_shader_view,
    create_output_view: rsf_d3d11_output_create_rtv,
    save: rsf_d3d11_state_save,
    restore: rsf_d3d11_state_restore,
    clear,
    copy: rsf_d3d11_copy_resource,
    draw,
};

pub(crate) fn rsf_auxiliary_sr_platform() -> *const AuxPlatform {
    &PLATFORM
}
