// SPDX-License-Identifier: MIT
//! Guarded access to the engine profile and the SDK draw slots.
use super::abi::{PositionViews, Raw};
use crate::engine_guards::{Read, read_bytes};
use crate::motion_target_abi::rsf_anomaly_motion_bind_layout as BindLayout;
use crate::native_profile::PROFILES as NATIVE;
use crate::sr_profile::{
    BACKEND_RT_CHANGED_OFFSET, BACKEND_RT_OFFSET, BACKEND_XFORMS_OFFSET, BACKEND_ZB_OFFSET,
    CRT_RTV_OFFSET, CRT_SURFACE_OFFSET, PROFILES as SR, RENDER_TARGET_OFFSET, TARGET_SLOTS,
    XFORMS_PROJECT_OFFSET, XFORMS_VIEW_OFFSET,
};
use std::ptr::{null_mut, with_exposed_provenance_mut};
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext_Vtbl};

/// rsf_anomaly_motion_engine: the installed build's engine addresses the native leaf reads, image-relative.
#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Engine {
    pub build: usize,
    pub hw_device_rva: u32,
    pub hw_context_rva: u32,
    pub backend_rva: u32,
    pub render_target_rva: u32,
    pub options_word3_rva: u32,
    pub device_frame_rva: u32,
    pub position_offset: u32,
    pub crt_rtv_offset: u32,
    pub crt_surface_offset: u32,
    pub view_rva: u32,
    pub projection_rva: u32,
}

const _: () = {
    assert!(size_of::<Engine>() == 56);
    assert!(std::mem::offset_of!(Engine, hw_device_rva) == 8);
    assert!(std::mem::offset_of!(Engine, projection_rva) == 48);
};

/// CBackend pRT/pZB and m_bChangedRTorZB, for the RTV2 binder.
pub(super) const BIND_LAYOUT: BindLayout = BindLayout {
    rt_offset: BACKEND_RT_OFFSET,
    zb_offset: BACKEND_ZB_OFFSET,
    changed_offset: BACKEND_RT_CHANGED_OFFSET,
};

impl Engine {
    /// The addresses of build `index` (native_profile.rs order, shared by sr_profile.rs).
    pub(super) fn of(index: usize) -> Option<Self> {
        let (native, sr) = (NATIVE.get(index)?, SR.get(index)?);
        // CRenderTarget::rt_Position, the G-buffer position target the window follows.
        let position = &TARGET_SLOTS[0];
        if position.name != c"$user$position" {
            return None;
        }
        let xforms = sr.backend_rva.checked_add(BACKEND_XFORMS_OFFSET)?;
        Some(Self {
            build: index,
            hw_device_rva: sr.hw_device_rva,
            hw_context_rva: sr.hw_context_rva,
            backend_rva: sr.backend_rva,
            render_target_rva: sr
                .render_implementation_rva
                .checked_add(RENDER_TARGET_OFFSET)?,
            options_word3_rva: sr.render_options_word3_rva,
            device_frame_rva: native.device_frame_rva,
            position_offset: position.offset,
            crt_rtv_offset: CRT_RTV_OFFSET,
            crt_surface_offset: CRT_SURFACE_OFFSET,
            view_rva: xforms.checked_add(XFORMS_VIEW_OFFSET)?,
            projection_rva: xforms.checked_add(XFORMS_PROJECT_OFFSET)?,
        })
    }
}

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
