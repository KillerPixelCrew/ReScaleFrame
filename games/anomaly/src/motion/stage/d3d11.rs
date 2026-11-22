// SPDX-License-Identifier: MIT
use super::abi::{Raw, ShaderObjects, TargetFacts};
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::*;
use windows::core::{IUnknown, Interface};

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_target_facts(raw: Raw, out: *mut TargetFacts) -> bool {
    let Some(surface) = (unsafe { IUnknown::from_raw_borrowed(&raw) }) else {
        return false;
    };
    let Ok(texture) = surface.cast::<ID3D11Texture2D>() else {
        return false;
    };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { texture.GetDesc(&mut desc) };
    unsafe {
        out.write(TargetFacts {
            width: desc.Width,
            height: desc.Height,
            samples: desc.SampleDesc.Count,
            layers: desc.ArraySize,
        });
    }
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_bound_shaders(
    raw: Raw,
    out: &mut ShaderObjects,
    all: bool,
) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let vtable = context.vtable();
    let get = |get: unsafe extern "system" fn(Raw, *mut Raw, *mut Raw, *mut u32)| {
        let mut shader = null_mut();
        unsafe { get(raw, &mut shader, null_mut(), null_mut()) };
        if !shader.is_null() {
            // The context still retains the stock binding during the flush.
            drop(unsafe { IUnknown::from_raw(shader) });
        }
        shader
    };
    let vs = get(vtable.VSGetShader);
    *out = if all {
        ShaderObjects {
            vs,
            hs: get(vtable.HSGetShader),
            ds: get(vtable.DSGetShader),
            ps: get(vtable.PSGetShader),
            gs: get(vtable.GSGetShader),
        }
    } else {
        ShaderObjects {
            vs,
            ..ShaderObjects::default()
        }
    };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_set_shaders(raw: Raw, shaders: &ShaderObjects, tess: bool) {
    let context = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }.unwrap();
    let vtable = context.vtable();
    unsafe {
        (vtable.VSSetShader)(raw, shaders.vs, std::ptr::null(), 0);
        (vtable.PSSetShader)(raw, shaders.ps, std::ptr::null(), 0);
        if tess {
            (vtable.HSSetShader)(raw, shaders.hs, std::ptr::null(), 0);
            (vtable.DSSetShader)(raw, shaders.ds, std::ptr::null(), 0);
        }
    }
}
