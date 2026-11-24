// SPDX-License-Identifier: MIT
//! The motion stage's engine reads and hook sets. motion_stage_platform.c keeps the detours, their unwind scopes and
//! the SEH-guarded engine includer; everything else reads engine memory through the guarded copy.
use super::abi::{DrawState, PositionViews, Raw};
use crate::engine_guards::{guarded_copy, read_bytes};
use crate::native_geometry_abi::NativeDrawFacts;
use crate::native_hooks::{self, HookSet, Site, Target};
use crate::native_profile::PROFILES as NATIVE;
use crate::sr_profile::{
    BACKEND_RT_CHANGED_OFFSET, BACKEND_RT_OFFSET, BACKEND_XFORMS_OFFSET, BACKEND_ZB_OFFSET,
    CRT_RTV_OFFSET, CRT_SURFACE_OFFSET, PROFILES as SR, RENDER_TARGET_OFFSET, TARGET_SLOTS,
    XFORMS_PROJECT_OFFSET, XFORMS_VIEW_OFFSET,
};
use crate::sys::rsf_anomaly_motion_bind_layout as BindLayout;
use std::ffi::c_void;
use std::ptr::{null_mut, with_exposed_provenance, with_exposed_provenance_mut};
use std::sync::OnceLock;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext_Vtbl};

/// The installed build's engine addresses, image-relative.
#[derive(Clone, Copy)]
pub(super) struct Engine {
    build: usize,           // native_profile.rs build index, for the geometry view read
    hw_device_rva: u32,     // HW.pDevice
    hw_context_rva: u32,    // HW.pContext
    backend_rva: u32,       // RCache (CBackend)
    render_target_rva: u32, // RImplementation.Target (CRenderTarget*)
    options_word3_rva: u32, // fourth u32 of CRender::o
    device_frame_rva: u32,  // Device.dwFrame
    position_offset: u32,   // CRenderTarget::rt_Position
    view_rva: u32,          // RCache.xforms.m_v
    projection_rva: u32,    // RCache.xforms.m_p
}
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
            view_rva: xforms.checked_add(XFORMS_VIEW_OFFSET)?,
            projection_rva: xforms.checked_add(XFORMS_PROJECT_OFFSET)?,
        })
    }
}

/// The image and engine addresses of the first install; trampolines pin them for the process.
static INSTALLED: OnceLock<(usize, Engine)> = OnceLock::new();

fn global<const N: usize>(rva: impl FnOnce(&Engine) -> u32) -> Option<[u8; N]> {
    let (image, engine) = INSTALLED.get()?;
    read_bytes(guarded_copy, image.checked_add(rva(engine) as usize)?)
}

fn global_pointer(rva: impl FnOnce(&Engine) -> u32) -> Raw {
    global(rva).map_or(null_mut(), |bytes| {
        with_exposed_provenance_mut(usize::from_ne_bytes(bytes))
    })
}

fn read_pointer(base: usize, offset: usize) -> Option<Raw> {
    if base == 0 {
        return None;
    }
    let value = usize::from_ne_bytes(read_bytes(guarded_copy, base.checked_add(offset)?)?);
    (value != 0).then(|| with_exposed_provenance_mut(value))
}

fn matrix(rva: impl FnOnce(&Engine) -> u32) -> Option<[f32; 16]> {
    let bytes: [u8; 64] = global(rva)?;
    let (words, _) = bytes.as_chunks::<4>();
    Some(std::array::from_fn(|i| f32::from_ne_bytes(words[i])))
}

pub(super) fn shader_device() -> Raw {
    global_pointer(|engine| engine.hw_device_rva)
}

pub(super) fn context() -> Raw {
    global_pointer(|engine| engine.hw_context_rva)
}

pub(super) fn render_target() -> Raw {
    global_pointer(|engine| engine.render_target_rva)
}

pub(super) fn backend() -> Raw {
    INSTALLED.get().map_or(null_mut(), |(image, engine)| {
        with_exposed_provenance_mut(image + engine.backend_rva as usize)
    })
}

pub(super) fn native_frame() -> Option<u32> {
    global(|engine| engine.device_frame_rva).map(u32::from_ne_bytes)
}

pub(super) fn options_word3() -> Option<u32> {
    global(|engine| engine.options_word3_rva).map(u32::from_ne_bytes)
}

/// RCache.xforms' view and projection.
pub(super) fn camera() -> Option<([f32; 16], [f32; 16])> {
    Some((
        matrix(|engine| engine.view_rva)?,
        matrix(|engine| engine.projection_rva)?,
    ))
}

/// The geometry producer's current render view.
pub(super) fn render_view() -> Option<u8> {
    let (image, engine) = INSTALLED.get()?;
    let view = unsafe { leaf::current_view(with_exposed_provenance(*image), engine.build) };
    (view != crate::native_geometry_abi::UNSET).then_some(view)
}

/// CRenderTarget::rt_Position's CRT pRT and pSurface.
pub(super) fn position_views(target: Raw) -> Option<PositionViews> {
    let (_, engine) = INSTALLED.get()?;
    let crt = read_pointer(target.addr(), engine.position_offset as usize)?;
    Some(PositionViews {
        rtv: read_pointer(crt.addr(), CRT_RTV_OFFSET as usize)?,
        surface: read_pointer(crt.addr(), CRT_SURFACE_OFFSET as usize)?,
    })
}

fn resolve_draws(context: Raw) -> Option<[Raw; 4]> {
    let vtable = read_pointer(context.expose_provenance(), 0)?;
    let offsets = [
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawIndexed),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, Draw),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawIndexedInstanced),
        std::mem::offset_of!(ID3D11DeviceContext_Vtbl, DrawInstanced),
    ];
    let mut methods = [null_mut(); 4];
    for (slot, offset) in methods.iter_mut().zip(offsets) {
        *slot = read_pointer(vtable.expose_provenance(), offset)?;
    }
    Some(methods)
}

static HOOKS: HookSet = HookSet::new(null_mut());
static DRAW_HOOKS: HookSet = HookSet::new(null_mut());

pub(super) fn hooks_created() -> bool {
    native_hooks::created(&HOOKS)
}

pub(super) fn disable_hooks() -> bool {
    native_hooks::set_enabled(&HOOKS, false)
}

/// Adopts the engine addresses, then installs the profile's sites (SITE_* order) with the leaf's detours.
pub(super) fn install_hooks(image: usize, size: u32, engine: Engine, sites: &[Site; 3]) -> bool {
    INSTALLED.get_or_init(|| (image, engine));
    if hooks_created() {
        return native_hooks::set_enabled(&HOOKS, true);
    }
    let (detours, originals) = leaf::detours();
    let sites: [Site; 3] = std::array::from_fn(|i| Site {
        replacement: detours[i],
        original: originals.wrapping_add(i),
        ..sites[i]
    });
    native_hooks::install(&HOOKS, with_exposed_provenance_mut(image), size, &sites)
}

/// The immediate context's four draw methods, resolved through its vtable.
pub(super) fn install_draw_hooks(context: Raw) -> bool {
    let Some(targets) = resolve_draws(context) else {
        return false;
    };
    let (detours, originals) = leaf::draw_detours();
    let sites: [Target; 4] = std::array::from_fn(|i| Target {
        address: targets[i],
        replacement: detours[i],
        original: originals.wrapping_add(i),
    });
    native_hooks::install_resolved(&DRAW_HOOKS, &sites)
}

unsafe extern "C" fn flush(facts: *const NativeDrawFacts) {
    // The geometry flush lends a live stack record for this call.
    if let Some(facts) = unsafe { facts.as_ref() } {
        super::rsf_anomaly_motion_stage_flush(facts);
    }
}

pub(super) fn observe_flush(enabled: bool) {
    crate::native_geometry::rsf_anomaly_geometry_set_flush(enabled.then_some(flush as _));
}

pub(super) fn read_engine(address: usize, bytes: &mut [u8]) -> bool {
    unsafe {
        guarded_copy(
            bytes.as_mut_ptr().cast(),
            with_exposed_provenance(address),
            bytes.len(),
        )
    }
}

unsafe extern "C" fn bind_read(_: Raw, address: *const c_void, out: Raw, size: usize) -> u32 {
    u32::from(unsafe { guarded_copy(out, address, size) })
}

unsafe extern "C" fn bind_write(_: Raw, address: Raw, input: *const c_void, size: usize) -> u32 {
    u32::from(unsafe { guarded_copy(address, input, size) })
}

/// The RTV2 binder's engine calls: the guarded copies and the motion device's target bindings.
pub(in crate::motion) static BIND: crate::motion_target::Platform =
    crate::motion_target::Platform {
        read: bind_read,
        write: bind_write,
        set_targets: crate::motion_device::rsf_anomaly_motion_bind_targets,
        clear_targets: crate::motion_device::rsf_anomaly_motion_clear_targets,
    };
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

/// The C leaves: motion_stage_platform.c's detours and trampoline slots and the geometry producer's reads. Unit tests
/// never reach the motion stage's native side, so they link stand-ins.
#[cfg(not(test))]
pub(super) mod leaf {
    use super::Raw;
    use std::ffi::c_void;

    unsafe extern "C" {
        static rsf_anomaly_motion_detours: [Raw; 3];
        static mut rsf_anomaly_motion_originals: [Raw; 3];
        static rsf_anomaly_motion_draw_detours: [Raw; 4];
        static mut rsf_anomaly_motion_draw_originals: [Raw; 4];
        #[link_name = "rsf_anomaly_geometry_current_view"]
        pub(super) fn current_view(module: *const c_void, build: usize) -> u8;
        #[link_name = "rsf_anomaly_geometry_world_matrix"]
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
    }

    pub(super) fn detours() -> (&'static [Raw; 3], *mut Raw) {
        unsafe {
            (
                &*(&raw const rsf_anomaly_motion_detours),
                (&raw mut rsf_anomaly_motion_originals).cast(),
            )
        }
    }

    pub(super) fn draw_detours() -> (&'static [Raw; 4], *mut Raw) {
        unsafe {
            (
                &*(&raw const rsf_anomaly_motion_draw_detours),
                (&raw mut rsf_anomaly_motion_draw_originals).cast(),
            )
        }
    }
}

#[cfg(test)]
pub(super) mod leaf {
    use super::Raw;
    use std::ffi::c_void;

    pub(super) fn detours() -> (&'static [Raw; 3], *mut Raw) {
        unreachable!("motion C leaf in a unit test")
    }
    pub(super) fn draw_detours() -> (&'static [Raw; 4], *mut Raw) {
        unreachable!("motion C leaf in a unit test")
    }
    pub(super) unsafe fn current_view(_: *const c_void, _: usize) -> u8 {
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
}
