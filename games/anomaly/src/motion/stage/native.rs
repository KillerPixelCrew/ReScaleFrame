// SPDX-License-Identifier: MIT
//! Guarded access to the engine profile and the SDK draw slots.
use super::abi::{PositionViews, Raw};
use crate::engine_guards::{Read, read_bytes};
use std::ptr::{null_mut, with_exposed_provenance_mut};
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext_Vtbl};

fn read_pointer(read: Read, base: usize, offset: usize) -> Option<Raw> {
    if base == 0 {
        return None;
    }
    let value = usize::from_ne_bytes(read_bytes(read, base.checked_add(offset)?)?);
    (value != 0).then(|| with_exposed_provenance_mut(value))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_resolve_draws(
    read: Read,
    context: Raw,
    out: &mut [Raw; 4],
) -> bool {
    let Some(vtable) = read_pointer(read, context.expose_provenance(), 0) else {
        return false;
    };
    let offsets = [
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawIndexed),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, Draw),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawIndexedInstanced),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawInstanced),
    ];
    let mut methods = [null_mut(); 4];
    for (slot, offset) in methods.iter_mut().zip(offsets) {
        let Some(function) = read_pointer(read, vtable.expose_provenance(), offset) else {
            return false;
        };
        *slot = function;
    }
    *out = methods;
    true
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_position_views(
    read: Read,
    target: Raw,
    position: u32,
    rtv: u32,
    surface: u32,
    out: &mut std::mem::MaybeUninit<PositionViews>,
) -> bool {
    let Some(crt) = read_pointer(read, target.expose_provenance(), position as usize) else {
        return false;
    };
    let (Some(rtv), Some(surface)) = (
        read_pointer(read, crt.expose_provenance(), rtv as usize),
        read_pointer(read, crt.expose_provenance(), surface as usize),
    ) else {
        return false;
    };
    out.write(PositionViews { rtv, surface });
    true
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_motion_viewport(raw: Raw, out: &mut [f32; 4]) {
    use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
    use windows::core::Interface;

    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) }) else {
        *out = [0.0; 4];
        return;
    };
    let mut viewport = D3D11_VIEWPORT::default();
    let mut count = 1;
    unsafe { (context.vtable().RSGetViewports)(raw, &mut count, &mut viewport) };
    *out = [
        viewport.TopLeftX,
        viewport.TopLeftY,
        viewport.Width,
        viewport.Height,
    ];
}
