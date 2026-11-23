// SPDX-License-Identifier: MIT
//! Guarded access to the engine profile and the SDK draw slots.
use super::abi::{DrawState, PositionViews, Raw};
use crate::engine_guards::{Read, read_bytes};
use crate::native_profile::PROFILES as NATIVE;
use crate::sr_profile::{
    BACKEND_RT_CHANGED_OFFSET, BACKEND_RT_OFFSET, BACKEND_XFORMS_OFFSET, BACKEND_ZB_OFFSET,
    CRT_RTV_OFFSET, CRT_SURFACE_OFFSET, PROFILES as SR, RENDER_TARGET_OFFSET, TARGET_SLOTS,
    XFORMS_PROJECT_OFFSET, XFORMS_VIEW_OFFSET,
};
use crate::sys::rsf_anomaly_motion_bind_layout as BindLayout;
use std::ptr::{null_mut, with_exposed_provenance_mut};
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext_Vtbl};

/// The installed build's engine addresses the native leaf reads, image-relative (motion_stage_platform.h).
pub(super) use crate::sys::rsf_anomaly_motion_engine as Engine;

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

/// The bound viewport of `raw` and the SR stage's current and previous jitter in viewport UV.
pub(super) unsafe fn draw_state(raw: Raw) -> DrawState {
    use windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext;
    use windows::core::Interface;

    let mut state = DrawState::default();
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&raw) } {
        let mut viewport = D3D11_VIEWPORT::default();
        let mut count = 1;
        unsafe { (context.vtable().RSGetViewports)(raw, &mut count, &mut viewport) };
        state.viewport = [
            viewport.TopLeftX,
            viewport.TopLeftY,
            viewport.Width,
            viewport.Height,
        ];
    }
    let jitter = crate::sr_stage::STAGE.jitter_uv();
    state.jitter = [
        jitter.current_u,
        jitter.current_v,
        jitter.previous_u,
        jitter.previous_v,
    ];
    state
}
