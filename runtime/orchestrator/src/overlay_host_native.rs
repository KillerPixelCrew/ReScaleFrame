// SPDX-License-Identifier: MIT
use super::*;
use crate::overlay_input::{
    rsf_overlay_input_collect, rsf_overlay_input_install, rsf_overlay_input_set_visible,
    rsf_overlay_input_uninstall, rsf_overlay_input_visible,
};
use crate::overlay_renderer11 as renderer;
use crate::sys::{RSF_OVERLAY_INPUT_ABI_VERSION, rsf_overlay_input_options as Options};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_TEXTURE2D_DESC, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceChild,
    ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::{IUnknown, Interface};
use windows_sys::Win32::System::{
    Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    SystemInformation::GetTickCount64,
    Threading::GetCurrentThreadId,
};

const E_POINTER: i32 = 0x8000_4003_u32 as i32;

fn hresult<T: Interface>(result: windows::core::Result<T>, out: *mut Handle) -> i32 {
    match result {
        Ok(value) => {
            unsafe { out.write(value.into_raw()) };
            0
        }
        Err(error) => error.code().0,
    }
}

unsafe extern "C" fn chain_device(chain: Handle, out: *mut Handle) -> i32 {
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return E_POINTER;
    };
    hresult(unsafe { chain.GetDevice::<ID3D11Device>() }, out)
}

unsafe extern "C" fn chain_buffer(chain: Handle, out: *mut Handle) -> i32 {
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return E_POINTER;
    };
    hresult(unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }, out)
}

unsafe extern "C" fn chain_window(chain: Handle) -> Handle {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDesc() }.ok())
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

unsafe extern "C" fn same_object(first: Handle, second: Handle) -> u32 {
    let identity = |object: Handle| {
        unsafe { IUnknown::from_raw_borrowed(&object) }
            .and_then(|object| object.cast::<IUnknown>().ok())
    };
    u32::from(identity(first).is_some_and(|a| identity(second).is_some_and(|b| a.as_raw() == b.as_raw())))
}

unsafe extern "C" fn immediate_context(device: Handle) -> Handle {
    unsafe { ID3D11Device::from_raw_borrowed(&device) }
        .and_then(|device| unsafe { device.GetImmediateContext() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn texture_device(texture: Handle) -> Handle {
    unsafe { ID3D11DeviceChild::from_raw_borrowed(&texture) }
        .and_then(|texture| unsafe { texture.GetDevice() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

unsafe extern "C" fn texture_desc(texture: Handle, out: *mut TextureDesc) {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    if let Some(texture) = unsafe { ID3D11Texture2D::from_raw_borrowed(&texture) } {
        unsafe { texture.GetDesc(&mut desc) };
    }
    unsafe {
        out.write(TextureDesc {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            bind_flags: desc.BindFlags,
        })
    };
}

unsafe extern "C" fn create_view(device: Handle, texture: Handle, out: *mut Handle) -> i32 {
    let (Some(device), Some(texture)) = (unsafe { ID3D11Device::from_raw_borrowed(&device) }, unsafe {
        ID3D11Resource::from_raw_borrowed(&texture)
    }) else {
        return E_POINTER;
    };
    let mut view = None;
    let result = unsafe { device.CreateRenderTargetView(texture, None, Some(&mut view)) };
    match (result, view) {
        (Ok(()), Some(view)) => {
            unsafe { out.write(view.into_raw()) };
            0
        }
        (Err(error), _) => error.code().0,
        (Ok(()), None) => E_POINTER,
    }
}

unsafe extern "C" fn save_targets(context: Handle, out: *mut Targets) {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return;
    };
    let mut views: [Option<ID3D11RenderTargetView>; 8] = Default::default();
    let mut depth = None;
    unsafe { context.OMGetRenderTargets(Some(&mut views), Some(&mut depth)) };
    let out = unsafe { &mut *out };
    for (slot, view) in out.views.iter_mut().zip(views) {
        *slot = view.map_or(null_mut(), Interface::into_raw);
    }
    out.depth = depth.map_or(null_mut(), Interface::into_raw);
}

unsafe extern "C" fn bind_target(context: Handle, view: Handle) {
    if let Some(context) = unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) } {
        let view = unsafe { ID3D11RenderTargetView::from_raw_borrowed(&view) }.cloned();
        unsafe { context.OMSetRenderTargets(Some(&[view]), None) };
    }
}

unsafe extern "C" fn restore_targets(context: Handle, saved: *const Targets) {
    let Some(context) = (unsafe { ID3D11DeviceContext::from_raw_borrowed(&context) }) else {
        return;
    };
    let saved = unsafe { &*saved };
    let views = saved
        .views
        .map(|view| unsafe { ID3D11RenderTargetView::from_raw_borrowed(&view) }.cloned());
    let depth = unsafe { ID3D11DepthStencilView::from_raw_borrowed(&saved.depth) };
    unsafe { context.OMSetRenderTargets(Some(&views), depth) };
}
unsafe extern "C" fn input_install(window: Handle, toggle: u32, log: Log, user: Handle) -> i32 {
    let options = Options {
        struct_size: size_of::<Options>() as u32,
        abi_version: RSF_OVERLAY_INPUT_ABI_VERSION,
        toggle_virtual_key: toggle,
        log,
        log_user: user,
    };
    unsafe { rsf_overlay_input_install(window, &options) }
}

unsafe extern "C" fn input_uninstall() {
    rsf_overlay_input_uninstall();
}

unsafe extern "C" fn input_collect(
    out: *mut rsf_overlay_input,
    width: u32,
    height: u32,
    delta: f32,
) {
    unsafe { rsf_overlay_input_collect(out, width, height, delta) };
}

unsafe extern "C" fn tick() -> u64 {
    unsafe { GetTickCount64() }
}

unsafe extern "C" fn counter(out: *mut i64) -> u32 {
    u32::from(unsafe { QueryPerformanceCounter(out) } != 0)
}

unsafe extern "C" fn frequency(out: *mut i64) -> u32 {
    u32::from(unsafe { QueryPerformanceFrequency(out) } != 0)
}

unsafe extern "C" fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

static PLATFORM: Platform = Platform {
    chain_device,
    chain_buffer,
    chain_window,
    same_object,
    immediate_context,
    texture_device,
    texture_desc,
    create_view,
    save_targets,
    bind_target,
    restore_targets,
    renderer_create: renderer::create,
    renderer_upload: renderer::upload_texture,
    renderer_free: renderer::free_texture,
    renderer_draw: renderer::draw,
    renderer_destroy: renderer::destroy,
    input_install,
    input_uninstall,
    input_visible: rsf_overlay_input_visible,
    input_set_visible: rsf_overlay_input_set_visible,
    input_collect,
    tick,
    counter,
    frequency,
    thread_id,
};

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_start(
    chain: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    unsafe { rsf_core_overlay_host_start(&PLATFORM, chain, log, user) }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_start_device(
    device: Handle,
    window: Handle,
    log: Log,
    user: Handle,
) -> i32 {
    unsafe { rsf_core_overlay_host_start_device(&PLATFORM, device, window, log, user) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_overlay_host_draw_target(
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    unsafe {
        rsf_core_overlay_host_draw_target(
            &PLATFORM, context, target, width, height, stats, intent, None,
        )
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_admit_target(
    show_hud: u32,
    visible: *mut u32,
    hint: *mut f32,
) -> i32 {
    if visible.is_null() || hint.is_null() || !STARTED.load(Ordering::Acquire) {
        return 0;
    }
    let Some(admission) = try_hold().and_then(|host| host.admit_target(&PLATFORM, show_hud != 0))
    else {
        return 0;
    };
    unsafe {
        visible.write(u32::from(admission.visible));
        hint.write(admission.hint);
    }
    1
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_draw_admitted_target(
    context: Handle,
    target: Handle,
    width: u32,
    height: u32,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    visible: u32,
    hint: f32,
) -> i32 {
    let admission = DrawAdmission {
        visible: visible != 0,
        hint,
    };
    unsafe {
        rsf_core_overlay_host_draw_target(
            &PLATFORM,
            context,
            target,
            width,
            height,
            stats,
            intent,
            Some(admission),
        )
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_host_visible() -> u32 {
    rsf_core_overlay_host_visible(&PLATFORM)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_host_toggle() {
    rsf_core_overlay_host_toggle(&PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_host_present(
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
) -> i32 {
    if !unsafe { crate::sr_session::has_capacity(stats) } {
        return 0;
    }
    let mut stats = unsafe { stats.read() };
    if !stats.saved_rows.is_null() {
        return unsafe { rsf_core_overlay_host_present(&PLATFORM, chain, &stats, intent) };
    }
    // Linked carriers that present directly still have the session-wide rows.
    let saved = crate::overlay_saved::Frame::new(null_mut());
    saved.attach(&mut stats);
    let result = unsafe { rsf_core_overlay_host_present(&PLATFORM, chain, &stats, intent) };
    if result != 0
        && let Some(intent) = unsafe { intent.as_ref() }
    {
        saved.apply(intent);
    }
    result
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_overlay_host_stop() {
    rsf_core_overlay_host_stop(&PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_overlay_host_stop_device(device: Handle) {
    rsf_core_overlay_host_stop_device(&PLATFORM, device);
}
