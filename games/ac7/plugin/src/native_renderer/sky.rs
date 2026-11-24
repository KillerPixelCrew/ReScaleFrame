// SPDX-License-Identifier: MIT
//! TrueSky: the depth producer dispatch relay and its one-to-one depth shader, the composite_tile
//! draw relay that reports cloud depth, the relay memory and module lookup the TrueSky owner
//! (truesky.rs) uses, and the sky render and projection hooks.
use super::{Guard, Renderer, hook, installed, key, original, primary_view, read, scope};
use crate::cloud_depth::d3d11::{CloudDepth, depth_shader};
use crate::truesky;
use rsf_game_sys::RSF_GAME_RENDER_CLOUD_DEPTH;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BUFFER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC, D3D11_TEXTURE2D_DESC,
    D3D11_UAV_DIMENSION_TEXTURE2D, D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_VIEWPORT, ID3D11Buffer,
    ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11UnorderedAccessView,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R32G32_FLOAT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_FREE, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_READWRITE,
    VirtualAlloc, VirtualFree, VirtualQuery,
};
use windows::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
use windows::core::{Interface, PCWSTR};

/// The depth producer's bound resources and their descriptions, as read before its dispatch.
#[derive(Default)]
struct DepthBindings {
    source: Option<ID3D11ShaderResourceView>,
    target: Option<ID3D11UnorderedAccessView>,
    constants: Option<ID3D11Buffer>,
    src: D3D11_TEXTURE2D_DESC,
    dst: D3D11_TEXTURE2D_DESC,
    cb: D3D11_BUFFER_DESC,
    srv: D3D11_SHADER_RESOURCE_VIEW_DESC,
    uav: D3D11_UNORDERED_ACCESS_VIEW_DESC,
}

impl DepthBindings {
    /// Describes the bound textures and buffer; true when they are the measured b11/t1/u0
    /// Texture2D RG32F layout. Descriptions of resources that are not textures stay zero.
    fn describe(&mut self) -> bool {
        let (Some(source), Some(target), Some(constants)) =
            (&self.source, &self.target, &self.constants)
        else {
            return false;
        };
        let texture = |resource: windows::core::Result<_>| {
            resource.and_then(
                |resource: windows::Win32::Graphics::Direct3D11::ID3D11Resource| {
                    resource.cast::<ID3D11Texture2D>()
                },
            )
        };
        let (Ok(source_texture), Ok(target_texture)) = (
            texture(unsafe { source.GetResource() }),
            texture(unsafe { target.GetResource() }),
        ) else {
            return false;
        };
        unsafe {
            source_texture.GetDesc(&mut self.src);
            target_texture.GetDesc(&mut self.dst);
            constants.GetDesc(&mut self.cb);
            source.GetDesc(&mut self.srv);
            target.GetDesc(&mut self.uav);
        }
        self.src.SampleDesc.Count == 1
            && self.src.Width > 0
            && self.src.Height > 0
            && self.cb.ByteWidth == 352
            && self.srv.ViewDimension == D3D_SRV_DIMENSION_TEXTURE2D
            && unsafe { self.srv.Anonymous.Texture2D.MostDetailedMip } == 0
            && self.dst.SampleDesc.Count == 1
            && self.dst.ArraySize == 1
            && self.uav.ViewDimension == D3D11_UAV_DIMENSION_TEXTURE2D
            && unsafe { self.uav.Anonymous.Texture2D.MipSlice } == 0
            && self.dst.Format == DXGI_FORMAT_R32G32_FLOAT
            && self.uav.Format == DXGI_FORMAT_R32G32_FLOAT
    }
}

type Dispatch = unsafe extern "system" fn(*mut c_void, u32, u32, u32);

/// The depth producer's `Dispatch` (ID3D11DeviceContext vtable slot 41), relayed from TrueSky.
pub(crate) unsafe extern "system" fn native_cloud_depth_dispatch(
    context: *mut c_void,
    x: u32,
    y: u32,
    z: u32,
) {
    let _outer = Guard::outer();
    let _entry = Guard::entry();
    let forward: Dispatch =
        unsafe { super::function(*(*context.cast::<*const *mut c_void>()).add(41)) };
    let (Some(owner), Some(device_context)) = (installed(), unsafe {
        ID3D11DeviceContext::from_raw_borrowed(&context)
    }) else {
        unsafe { forward(context, x, y, z) };
        return;
    };
    let device = unsafe { device_context.GetDevice() }.ok();
    let device_raw = device.as_ref().map_or(null_mut(), Interface::as_raw);
    let shader = {
        let _lock = owner.cloud_guard();
        let cloud = owner.cloud.get();
        // A recreated D3D device cannot use the previous device's shader. Native texture
        // ownership supplies the replacement views; rebuild only our shader before validating
        // this dispatch.
        if let Some(previous) = unsafe { &(*cloud).device }
            && previous.as_raw() != device_raw
        {
            unsafe {
                (*cloud).shader = None;
                (*cloud).device = None;
                (*cloud).shader_refused = false;
            }
            owner.cloud_depth_ready.store(false, Ordering::Release);
        }
        if unsafe { (*cloud).shader.is_none() && !(*cloud).shader_refused } {
            let created = device.as_ref().and_then(depth_shader);
            if created.is_none() {
                unsafe { (*cloud).shader_refused = true };
                owner.log(c"native TrueSky one-to-one depth shader refused; retaining supported native cloud resolution");
            } else {
                unsafe {
                    (*cloud).shader = created;
                    (*cloud).device = device.clone();
                }
                owner.log(c"native TrueSky: one-to-one depth shader compiled; checking native pass bindings");
            }
        }
        let same_device = unsafe { (*cloud).device.as_ref() }
            .is_some_and(|current| current.as_raw() == device_raw);
        if same_device {
            unsafe { (*cloud).shader.clone() }
        } else {
            None
        }
    };
    // This call site belongs only to native screenspace depth production. Identify its active
    // resolution from the actual resources, including even padding, instead of guessing a view.
    let mut bound = DepthBindings::default();
    let (mut source, mut target, mut constants) = ([None], [None], [None]);
    unsafe {
        device_context.CSGetShaderResources(1, Some(&mut source));
        device_context.CSGetUnorderedAccessViews(0, Some(&mut target));
        device_context.CSGetConstantBuffers(11, Some(&mut constants));
    }
    [bound.source] = source;
    [bound.target] = target;
    [bound.constants] = constants;
    let mut bindings = shader.is_some() && bound.describe();
    let mut full = bindings
        && bound.dst.Width.abs_diff(bound.src.Width) <= 2
        && bound.dst.Height.abs_diff(bound.src.Height) <= 2;
    let mut original_shader = None;
    let mut classes = 0u32;
    if bindings {
        unsafe { device_context.CSGetShader(&mut original_shader, None, Some(&mut classes)) };
        bindings = original_shader.is_some() && classes == 0;
        full = full && bindings;
    }
    if bindings && !owner.cloud_depth_ready.swap(true, Ordering::AcqRel) {
        owner.log_bounded(
            &format!(
                "native TrueSky: b11/t1/u0 Texture2D RG32F depth bindings verified ({}x{} -> {}x{}); full cloud grid may activate",
                bound.src.Width, bound.src.Height, bound.dst.Width, bound.dst.Height
            ),
            192,
        );
    }
    if !bindings
        && owner
            .cloud_depth_binding_refusals
            .fetch_add(1, Ordering::SeqCst)
            < 4
    {
        owner.log_bounded(
            &format!(
                "native TrueSky depth binding refused: srv={} uav={} cb={}, SRVdim={} UAVdim={} targetFormat={} viewFormat={}, source={}x{} target={}x{} classes={}",
                u32::from(bound.source.is_some()),
                u32::from(bound.target.is_some()),
                bound.cb.ByteWidth,
                bound.srv.ViewDimension.0 as u32,
                bound.uav.ViewDimension.0 as u32,
                bound.dst.Format.0 as u32,
                bound.uav.Format.0 as u32,
                bound.src.Width,
                bound.src.Height,
                bound.dst.Width,
                bound.dst.Height,
                classes
            ),
            320,
        );
    }
    if !bindings {
        // Readiness of an earlier frame does not authorize running x2 on this frame's x1 targets.
        // Retire full-grid mode for the next native allocation and reject this unverified
        // dispatch. The enclosing native function still performs its balanced Unapply/cleanup.
        // The full-grid site is read under the lock that every change of it holds (truesky.rs).
        let _lock = owner.cloud_guard();
        if !unsafe { (*owner.truesky.get()).resolution_site }.is_null() {
            owner.cloud_depth_ready.store(false, Ordering::Release);
            truesky::refuse_full_grid(unsafe { &mut *owner.truesky.get() }, owner);
            return;
        }
    }
    if full {
        unsafe { device_context.CSSetShader(shader.as_ref(), None) };
        if owner.cloud_depth_dispatches.fetch_add(1, Ordering::SeqCst) < 3 {
            owner.log(c"native TrueSky: normalized one-to-one scene depth dispatched on native effect resources");
        }
    }
    unsafe { forward(context, x, y, z) };
    // Restore the exact pass shader before TrueSky's own Unapply, keeping its cached state valid.
    if full {
        unsafe { device_context.CSSetShader(original_shader.as_ref(), None) };
    }
}

/// Relay memory within rel32 reach of `call`, or null.
fn allocate_depth_relay(call: *mut u8) -> *mut c_void {
    let mut info = SYSTEM_INFO::default();
    unsafe { GetSystemInfo(&mut info) };
    let granularity = info.dwAllocationGranularity as usize;
    let centre = call as usize & !(granularity.wrapping_sub(1));
    let mut delta = granularity;
    while delta < 0x7000_0000 {
        let below = centre.saturating_sub(delta);
        for candidate in [centre.wrapping_add(delta), below] {
            if candidate == 0 {
                continue;
            }
            let mut region = MEMORY_BASIC_INFORMATION::default();
            let address = candidate as *const c_void;
            if unsafe {
                VirtualQuery(
                    Some(address),
                    &mut region,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            } != 0
                && region.State == MEM_FREE
            {
                let memory = unsafe {
                    VirtualAlloc(
                        Some(address),
                        4096,
                        MEM_RESERVE | MEM_COMMIT,
                        PAGE_READWRITE,
                    )
                };
                if !memory.is_null() {
                    return memory;
                }
            }
        }
        delta += granularity;
    }
    null_mut()
}

/// composite_tile reaches TrueSky's render platform at its discovered call (recorded +ABC6B),
/// `call [rax+0x130]` with (platform, deviceContext&, count). The relay forwards it, then reads
/// the pass's still-bound cloud inputs before TrueSky's Unapply and hands the host a device depth
/// for cloud motion.
pub(crate) unsafe extern "C" fn native_cloud_composite_draw(
    platform: *mut c_void,
    device_context: *mut c_void,
    count: u32,
) {
    let _outer = Guard::outer();
    let _entry = Guard::entry();
    type PlatformDraw = unsafe extern "C" fn(*mut c_void, *mut c_void, u32);
    let draw: PlatformDraw =
        unsafe { super::function(*(*platform.cast::<*const *mut c_void>()).add(0x130 / 8)) };
    unsafe { draw(platform, device_context, count) };
    let Some(owner) = installed().filter(|owner| owner.active()) else {
        return;
    };
    let Some(on_stage) = owner.on_stage() else {
        return;
    };
    let context = {
        let _lock = owner.cloud_guard();
        // Known only after TrueSky's depth producer ran.
        let Some(device) = (unsafe { &(*owner.cloud.get()).device }) else {
            return;
        };
        unsafe { device.GetImmediateContext() }.ok()
    };
    let Some(context) = context else {
        return;
    };
    let mut viewport = D3D11_VIEWPORT::default();
    let mut viewports = 1u32;
    unsafe { context.RSGetViewports(&mut viewports, Some(&mut viewport)) };
    // The render thread alone uses the cloud motion resources here (Renderer::cloud_motion).
    let depth = (viewports == 1)
        .then(|| {
            crate::cloud_depth::d3d11::write(
                unsafe { &mut *owner.cloud_motion.get() },
                &context,
                viewport.Width,
                viewport.Height,
            )
        })
        .flatten();
    let reports = owner.cloud_motion_reports.fetch_add(1, Ordering::SeqCst);
    let Some(depth) = depth else {
        if reports < 3 {
            owner.log(c"native TrueSky cloud motion refused: composite_tile bindings differ from the measured t1/t2/b12 layout");
        }
        return;
    };
    let mut pass = scope();
    pass.role = RSF_GAME_RENDER_CLOUD_DEPTH;
    pass.session_id = owner.options.session_id;
    pass.depth = depth;
    let (left, top) = (viewport.TopLeftX as i32, viewport.TopLeftY as i32);
    pass.render_rect = [
        left,
        top,
        left.wrapping_add(viewport.Width as i32),
        top.wrapping_add(viewport.Height as i32),
    ];
    unsafe { on_stage(owner.options.user, null_mut(), &pass, 1) };
    if reports < 3 {
        owner.log(c"native TrueSky cloud motion: composite_tile cloud distance converted to scene device depth");
    }
}

pub(crate) fn module(name: *const u16) -> *mut u8 {
    #[cfg(feature = "warp-fixture")]
    {
        let module = super::TEST_TRUESKY_MODULE.load(Ordering::Relaxed);
        if !module.is_null() {
            return module;
        }
    }
    unsafe { GetModuleHandleW(PCWSTR(name)) }.map_or(null_mut(), |module| module.0.cast())
}

pub(crate) fn relay_allocate(call: *mut u8, handler: *mut c_void) -> *mut c_void {
    let slot = allocate_depth_relay(call);
    if !slot.is_null() {
        unsafe { slot.cast::<*mut c_void>().write(handler) };
    }
    slot
}

pub(crate) fn relay_release(slot: *mut c_void) {
    if !slot.is_null() {
        let _ = unsafe { VirtualFree(slot, 0, MEM_RELEASE) };
    }
}

pub(crate) fn depth_ready(owner: &Renderer) -> bool {
    owner.cloud_depth_ready.load(Ordering::Acquire)
}

/// Called by truesky.rs with `cloud_lock` held, once no relay call runs.
pub(crate) fn motion_release(owner: &Renderer) {
    unsafe { *owner.cloud_motion.get() = CloudDepth::default() };
}

/// Called by truesky.rs with `cloud_lock` held.
pub(crate) fn depth_release(owner: &Renderer) {
    let cloud = owner.cloud.get();
    unsafe {
        (*cloud).shader = None;
        (*cloud).device = None;
    }
    owner.cloud_depth_ready.store(false, Ordering::SeqCst);
}

/// Installs the TrueSky relays and, once the depth relay is ready, the full cloud grid
/// (truesky.rs).
pub(super) fn apply_cloud_resolution_patch(owner: &Renderer) {
    let _lock = owner.cloud_guard();
    truesky::apply(unsafe { &mut *owner.truesky.get() }, owner);
}

/// Restores the TrueSky plugin patches and relays (truesky.rs) under the lock every change of
/// them holds, so the depth relay cannot retire the full grid while stop restores it.
pub(super) fn restore_truesky(owner: &Renderer) -> bool {
    let _lock = owner.cloud_guard();
    truesky::restore(unsafe { &mut *owner.truesky.get() }, owner)
}

thread_local! {
    static SKY_PROJECTION_JITTER: Cell<bool> = const { Cell::new(false) };
}

pub(super) unsafe extern "C" fn hooked_sky_projection(matrix: *mut f32, units: f32) {
    let _entry = Guard::entry();
    let forward: unsafe extern "C" fn(*mut f32, f32) = unsafe { original(hook::SKY_PROJECTION) };
    unsafe { forward(matrix, units) };
    // The native handedness conversion negates M20 and M23. M21 must follow M20
    // for temporal projection offsets to retain the scene's vertical sampling direction.
    if SKY_PROJECTION_JITTER.get() && !matrix.is_null() {
        unsafe { *matrix.add(9) = -*matrix.add(9) };
    }
}

static JITTER_REPORTS: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn hooked_sky_render(plugin: *mut c_void, parameters: *mut c_void) {
    let _outer = Guard::outer();
    let _entry = Guard::entry();
    let previous = SKY_PROJECTION_JITTER.replace(false);
    let owner = installed();
    let (mut view, mut cached) = (0u64, 0u64);
    let (mut jitter_y, mut projection_y, mut perspective) = (0f32, 0f32, 0f32);
    let parameters_key = key(parameters);
    if let Some(owner) = owner
        && owner.active()
    {
        apply_cloud_resolution_patch(owner);
    }
    if let Some(owner) = owner
        && owner.active()
        && let Some(config) = owner.config()
        && config.enabled != 0
        && read(parameters_key, 0xb0, &mut view)
        && primary_view(view, &config)
        && read(view, 0x1418, &mut cached)
        && read(cached, 0x724, &mut jitter_y)
        && read(parameters_key, 0x74, &mut projection_y)
        && read(parameters_key, 0x7c, &mut perspective)
        && jitter_y.is_finite()
        && jitter_y != 0.0
        && (projection_y - jitter_y).abs() < 1e-6
        && (perspective - 1.0).abs() < 1e-6
    {
        SKY_PROJECTION_JITTER.set(true);
        if JITTER_REPORTS.fetch_add(1, Ordering::Relaxed) < 4 {
            owner.log(c"native TrueSky: matched primary-view vertical jitter handedness corrected");
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void) =
        unsafe { original(hook::SKY_RENDER) };
    unsafe { forward(plugin, parameters) };
    SKY_PROJECTION_JITTER.set(previous);
}

#[cfg(feature = "warp-fixture")]
pub(super) fn composite_draw_handler() -> *mut c_void {
    native_cloud_composite_draw as *mut c_void
}
