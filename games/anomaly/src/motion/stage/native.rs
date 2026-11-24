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

/// The C leaves of motion_stage_platform.c and the geometry constant reads. Unit tests never reach the motion stage's
/// native side, so they link stand-ins.
#[cfg(not(test))]
pub(super) mod leaf {
    use super::super::abi::{PositionViews, Raw};
    use super::Engine;
    use std::ffi::c_void;

    unsafe extern "C" {
        #[link_name = "rsf_anomaly_motion_native_copy"]
        pub(in crate::motion) fn copy(out: *mut c_void, input: *const c_void, size: usize) -> bool;
        #[link_name = "rsf_anomaly_motion_native_observe_flush"]
        pub(in crate::motion) fn observe_flush(enabled: bool);
        #[link_name = "rsf_anomaly_motion_native_install_hooks"]
        pub(in crate::motion) fn install_hooks(
            image: Raw,
            size: u32,
            engine: *const Engine,
            sites: *const c_void,
            count: usize,
        ) -> bool;
        #[link_name = "rsf_anomaly_motion_native_disable_hooks"]
        pub(in crate::motion) fn disable_hooks() -> bool;
        #[link_name = "rsf_anomaly_motion_native_hooks_created"]
        pub(in crate::motion) fn hooks_created() -> bool;
        #[link_name = "rsf_anomaly_motion_native_install_draw_hooks"]
        pub(in crate::motion) fn install_draw_hooks(context: Raw) -> bool;
        #[link_name = "rsf_anomaly_motion_native_render_target"]
        pub(in crate::motion) fn render_target() -> Raw;
        #[link_name = "rsf_anomaly_motion_native_position_views"]
        pub(in crate::motion) fn position_views(target: Raw, views: *mut PositionViews) -> bool;
        #[link_name = "rsf_anomaly_motion_native_native_frame"]
        pub(in crate::motion) fn native_frame(frame: *mut u32) -> bool;
        #[link_name = "rsf_anomaly_motion_native_render_view"]
        pub(in crate::motion) fn render_view(view: *mut u8) -> bool;
        #[link_name = "rsf_anomaly_motion_native_options_word3"]
        pub(in crate::motion) fn options_word3(options: *mut u32) -> bool;
        #[link_name = "rsf_anomaly_motion_native_backend"]
        pub(in crate::motion) fn backend() -> Raw;
        #[link_name = "rsf_anomaly_motion_native_context"]
        pub(in crate::motion) fn context() -> Raw;
        #[link_name = "rsf_anomaly_motion_native_shader_device"]
        pub(in crate::motion) fn shader_device() -> Raw;
        #[link_name = "rsf_anomaly_motion_native_camera"]
        pub(in crate::motion) fn camera(view: *mut [f32; 16], projection: *mut [f32; 16]) -> bool;
        #[link_name = "rsf_anomaly_motion_native_world"]
        pub(in crate::motion) fn world(matrix: *mut [f32; 16]) -> bool;
        #[link_name = "rsf_anomaly_geometry_read_constants"]
        pub(in crate::motion) fn read_constant(
            stage: u32,
            slot: u32,
            offset: u32,
            out: *mut c_void,
            size: u32,
        ) -> bool;
        #[link_name = "rsf_anomaly_geometry_write_constants"]
        pub(in crate::motion) fn write_constant(
            stage: u32,
            slot: u32,
            offset: u32,
            input: *const c_void,
            size: u32,
        ) -> bool;
        #[link_name = "rsf_anomaly_motion_native_read_engine"]
        pub(in crate::motion) fn read_engine(
            owner: *const c_void,
            address: usize,
            out: *mut u8,
            size: usize,
        ) -> bool;
        #[link_name = "rsf_anomaly_motion_native_bind_read"]
        fn bind_read(host: Raw, address: *const c_void, out: Raw, size: usize) -> u32;
        #[link_name = "rsf_anomaly_motion_native_bind_write"]
        fn bind_write(host: Raw, address: Raw, input: *const c_void, size: usize) -> u32;
    }

    /// The RTV2 binder's engine calls: the leaf's guarded copies and the motion device's target bindings.
    pub(in crate::motion) static BIND: crate::motion_target::Platform =
        crate::motion_target::Platform {
            read: bind_read,
            write: bind_write,
            set_targets: crate::motion_device::rsf_anomaly_motion_bind_targets,
            clear_targets: crate::motion_device::rsf_anomaly_motion_clear_targets,
        };
}

#[cfg(test)]
pub(super) mod leaf {
    use super::super::abi::{PositionViews, Raw};
    use super::Engine;
    use std::ffi::c_void;

    pub(in crate::motion) unsafe extern "C" fn copy(
        _: *mut c_void,
        _: *const c_void,
        _: usize,
    ) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn observe_flush(_: bool) {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn install_hooks(
        _: Raw,
        _: u32,
        _: *const Engine,
        _: *const c_void,
        _: usize,
    ) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn disable_hooks() -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn hooks_created() -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn install_draw_hooks(_: Raw) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn render_target() -> Raw {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn position_views(_: Raw, _: *mut PositionViews) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn native_frame(_: *mut u32) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn render_view(_: *mut u8) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn options_word3(_: *mut u32) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn backend() -> Raw {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn context() -> Raw {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn shader_device() -> Raw {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn camera(_: *mut [f32; 16], _: *mut [f32; 16]) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn world(_: *mut [f32; 16]) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn read_constant(
        _: u32,
        _: u32,
        _: u32,
        _: *mut c_void,
        _: u32,
    ) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn write_constant(
        _: u32,
        _: u32,
        _: u32,
        _: *const c_void,
        _: u32,
    ) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) unsafe fn read_engine(
        _: *const c_void,
        _: usize,
        _: *mut u8,
        _: usize,
    ) -> bool {
        unreachable!("motion C leaf in a unit test")
    }
    unsafe extern "C" fn bind_read(_: Raw, _: *const c_void, _: Raw, _: usize) -> u32 {
        unreachable!("motion C leaf in a unit test")
    }
    unsafe extern "C" fn bind_write(_: Raw, _: Raw, _: *const c_void, _: usize) -> u32 {
        unreachable!("motion C leaf in a unit test")
    }
    pub(in crate::motion) static BIND: crate::motion_target::Platform =
        crate::motion_target::Platform {
            read: bind_read,
            write: bind_write,
            set_targets: crate::motion_device::rsf_anomaly_motion_bind_targets,
            clear_targets: crate::motion_device::rsf_anomaly_motion_clear_targets,
        };
}
