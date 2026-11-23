// SPDX-License-Identifier: MIT
//! D3D11 resource and replay calls. The engine and fixtures borrow the same tables.
use super::replay::{CLASS_INTERFACES, Depth, Pixel, Platform as ReplayPlatform, RENDER_TARGETS};
use super::{Log, Platform, Raw};
use crate::host_log::Log as HostLog;
use rsf_shader_compiler::{Compiler, Request, compile};
use std::ptr::{copy_nonoverlapping, null_mut};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};
use windows::core::{IUnknown, Interface};

const _: () = {
    assert!(RENDER_TARGETS == D3D11_SIMULTANEOUS_RENDER_TARGET_COUNT as usize);
    assert!(CLASS_INTERFACES == D3D11_SHADER_MAX_INTERFACES as usize);
};

unsafe extern "C" fn retain(raw: Raw) {
    let unknown = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (unknown.vtable().AddRef)(raw) };
}

unsafe extern "C" fn release(raw: Raw) {
    drop(unsafe { IUnknown::from_raw(raw) });
}

fn owned<T: Interface>(value: Option<T>) -> Raw {
    value.map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn buffer(raw: Raw, size: u32) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: size,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateBuffer(&desc, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

unsafe extern "C" fn zero_shader(raw: Raw, callback: Log, user: Raw) -> Raw {
    let log = HostLog {
        callback,
        user: user as usize,
    };
    let Some(compiler) = Compiler::system("d3dcompiler_47.dll") else {
        log.text(c"shader rsf_anomaly_zero_motion: d3dcompiler_47 is not available");
        return null_mut();
    };
    let Some(entry) = compiler.entry() else {
        log.text(c"shader rsf_anomaly_zero_motion: D3DCompile is not available");
        return null_mut();
    };
    let request = Request {
        source: b"float4 main() : SV_Target2 { return float4(0, 0, 0, 0); }",
        name: c"rsf_anomaly_zero_motion",
        entry: c"main",
        target: c"ps_5_0",
        flags: 1 << 11, // D3DCOMPILE_ENABLE_STRICTNESS
    };
    let compiled = unsafe { compile(entry, request, &[], null_mut()) };
    let Some(code) = compiled.code.filter(|_| compiled.status >= 0) else {
        let errors = compiled.errors.as_deref().unwrap_or_default();
        log.say(format!(
            "shader rsf_anomaly_zero_motion (main): compile failed 0x{:08x} {}",
            compiled.status as u32,
            String::from_utf8_lossy(errors).trim_end_matches('\0'),
        ));
        return null_mut();
    };
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let mut result = None;
    if unsafe { device.CreatePixelShader(&code, None::<&ID3D11ClassLinkage>, Some(&mut result)) }
        .is_err()
    {
        return null_mut();
    }
    owned(result)
}

unsafe extern "C" fn depth_state(raw: Raw, equal: u32) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: (equal != 0).into(),
        DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
        DepthFunc: if equal != 0 {
            D3D11_COMPARISON_EQUAL
        } else {
            D3D11_COMPARISON_ALWAYS
        },
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateDepthStencilState(&desc, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

unsafe extern "C" fn texture(raw: Raw, width: u32, height: u32) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    };
    let mut result = None;
    if unsafe { device.CreateTexture2D(&desc, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

unsafe extern "C" fn view(raw: Raw, texture: Raw) -> Raw {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&raw) }.unwrap();
    // ID3D11Texture2D inherits ID3D11Resource.
    let texture = unsafe { ID3D11Resource::from_raw_borrowed(&texture) }.unwrap();
    let mut result = None;
    if unsafe { device.CreateRenderTargetView(texture, None, Some(&mut result)) }.is_err() {
        return null_mut();
    }
    owned(result)
}

unsafe extern "C" fn upload(
    raw: Raw,
    buffer: Raw,
    data: *const std::ffi::c_void,
    size: usize,
) -> u32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let buffer = unsafe { ID3D11Resource::from_raw_borrowed(&buffer) }.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { context.Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped)) }.is_err() {
        return 0;
    }
    let valid = !mapped.pData.is_null();
    unsafe {
        if valid {
            copy_nonoverlapping(data.cast::<u8>(), mapped.pData.cast(), size);
        }
        context.Unmap(buffer, 0);
    }
    valid.into()
}

unsafe extern "C" fn bind(raw: Raw, buffer: Raw, stage: std::ffi::c_char) -> u32 {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let vtable = context.vtable();
    let bind = match stage as u8 {
        b'v' => vtable.VSSetConstantBuffers,
        b'p' => vtable.PSSetConstantBuffers,
        b'd' => vtable.DSSetConstantBuffers,
        _ => return 0,
    };
    unsafe { bind(raw, 13, 1, &buffer) };
    1
}

pub(crate) fn rsf_anomaly_motion_device_platform_get() -> &'static Platform {
    static PLATFORM: Platform = Platform {
        retain,
        release,
        buffer,
        zero_shader,
        depth_state,
        texture,
        view,
        upload,
        bind,
    };
    &PLATFORM
}

unsafe extern "C" fn read_depth(raw: Raw, out: &mut Depth) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let vtable = context.vtable();
    let mut state = None;
    unsafe {
        (vtable.OMGetRenderTargets)(
            raw,
            out.targets.len() as u32,
            out.targets.as_mut_ptr(),
            &mut out.view,
        );
        context.OMGetDepthStencilState(Some(&mut state), Some(&mut out.reference));
    }
    (out.enabled, out.writable) = state.as_ref().map_or((true, true), |state| {
        let mut desc = D3D11_DEPTH_STENCIL_DESC::default();
        unsafe { state.GetDesc(&mut desc) };
        (
            desc.DepthEnable.as_bool(),
            desc.DepthWriteMask != D3D11_DEPTH_WRITE_MASK_ZERO,
        )
    });
    out.state = owned(state);
}

unsafe extern "C" fn read_pixel(raw: Raw, out: &mut Pixel) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let mut blend = None;
    out.count = out.classes.len() as u32;
    unsafe {
        context.OMGetBlendState(Some(&mut blend), Some(&mut out.factor), Some(&mut out.mask));
        (context.vtable().PSGetShader)(
            raw,
            &mut out.shader,
            out.classes.as_mut_ptr(),
            &mut out.count,
        );
    }
    out.blend = owned(blend);
}

pub(crate) unsafe fn clear_target(raw: Raw, target: Raw) {
    let (Some(context), Some(target)) = (unsafe {
        (
            ID3D11DeviceContext::from_raw_borrowed(&raw),
            ID3D11RenderTargetView::from_raw_borrowed(&target),
        )
    }) else {
        return;
    };
    unsafe { context.ClearRenderTargetView(target, &[0.0; 4]) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_bind_targets(raw: Raw, views: *const Raw, depth: Raw) {
    if !raw.is_null() {
        unsafe { targets(raw, 4, views, depth) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_clear_targets(raw: Raw) {
    if !raw.is_null() {
        unsafe { targets(raw, 0, std::ptr::null(), null_mut()) };
    }
}

unsafe extern "C" fn targets(raw: Raw, count: u32, views: *const Raw, depth: Raw) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (context.vtable().OMSetRenderTargets)(raw, count, views, depth) };
}

unsafe extern "C" fn depth(raw: Raw, state: Raw, reference: u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (context.vtable().OMSetDepthStencilState)(raw, state, reference) };
}

unsafe extern "C" fn blend(raw: Raw, state: Raw, factor: *const f32, mask: u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (context.vtable().OMSetBlendState)(raw, state, factor, mask) };
}

unsafe extern "C" fn shader(raw: Raw, pixel: Raw, classes: *const Raw, count: u32) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (context.vtable().PSSetShader)(raw, pixel, classes, count) };
}

pub(crate) fn rsf_anomaly_motion_replay_platform_get() -> &'static ReplayPlatform {
    static PLATFORM: ReplayPlatform = ReplayPlatform {
        release,
        read_depth,
        read_pixel,
        targets,
        depth,
        blend,
        shader,
    };
    &PLATFORM
}
