// SPDX-License-Identifier: MIT
//! Package renderer routing and bridge retirement.
use super::*;
use crate::d3d11_state::{self, graphics as d3d11};
use crate::package_state::rsf_unity_package_sr_slots_released;
use crate::sys::*;
use std::sync::atomic::AtomicU32;
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::core::Interface;

static EVENT_ID: AtomicI32 = AtomicI32::new(0);
static RENDERER: AtomicU32 = AtomicU32::new(RSF_UNITY_BRIDGE_NONE);

unsafe extern "C" {
    fn rsf_unity_package_sdk_renderer(interfaces: *mut c_void, graphics: *mut *mut c_void) -> u32;
    fn rsf_unity_package_sdk_reserve_events(graphics: *mut c_void) -> c_int;
    fn rsf_unity_package_sdk_bind_d3d11(interfaces: *mut c_void);
    fn rsf_unity_package_sdk_unbind_d3d11();
    fn rsf_unity_package_sdk_has_d3d11() -> bool;
    fn rsf_unity_package_sdk_bind_vulkan(interfaces: *mut c_void, event: c_int);
    fn rsf_unity_package_sdk_unbind_vulkan();
    fn rsf_unity_d3d12_sdk_load(interfaces: *mut c_void, package: bool);
    fn rsf_unity_d3d12_sdk_unload();

    // vulkan_bridge.rs. The Unity calls behind them are guarded in vulkan_bridge.c.
    fn rsf_uvk_bridge_create_window() -> bool;
    fn rsf_uvk_bridge_render(
        event_id: c_int,
        base_event: c_int,
        packet: *const rsf_unity_packet,
        services: *const rsf_game_host_services,
        reconfigure: bool,
    ) -> bool;
    #[cfg_attr(test, allow(dead_code))]
    fn rsf_uvk_bridge_graphics(device: *mut *mut c_void, queue: *mut *mut c_void) -> bool;
    fn rsf_unity_vulkan_open();
    fn rsf_unity_vulkan_stop_admission();
    fn rsf_unity_vulkan_try_close() -> bool;
    fn rsf_unity_vulkan_finish_retirement(commit: bool);
    fn rsf_unity_preview_drained() -> bool;
    fn rsf_unity_preview_release();
    fn rsf_unity_package_editor() -> u32;
}

pub(super) fn event_id() -> c_int {
    EVENT_ID.load(SeqCst)
}

pub(super) fn route() -> Route {
    match RENDERER.load(SeqCst) {
        RSF_UNITY_BRIDGE_D3D11 => Route::D3d11,
        RSF_UNITY_BRIDGE_D3D12 => Route::D3d12,
        RSF_UNITY_BRIDGE_VULKAN => Route::Vulkan,
        _ => Route::None,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_bridge_load(interfaces: *mut c_void) -> u32 {
    if interfaces.is_null() {
        return 0;
    }
    let mut graphics = null_mut();
    let renderer = unsafe { rsf_unity_package_sdk_renderer(interfaces, &mut graphics) };
    if graphics.is_null() {
        return 1;
    }
    RENDERER.store(renderer, SeqCst);
    match route() {
        Route::D3d12 => unsafe { rsf_unity_d3d12_sdk_load(interfaces, true) },
        Route::D3d11 => unsafe {
            rsf_unity_package_sdk_bind_d3d11(interfaces);
            EVENT_ID.store(rsf_unity_package_sdk_reserve_events(graphics), SeqCst);
        },
        Route::Vulkan => unsafe {
            let event = rsf_unity_package_sdk_reserve_events(graphics);
            EVENT_ID.store(event, SeqCst);
            rsf_unity_package_sdk_bind_vulkan(interfaces, event);
        },
        Route::None => {}
    }
    1
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_bridge_unload() {
    if matches!(route(), Route::D3d12) {
        rsf_unity_frames_activate(0);
        unsafe { rsf_unity_d3d12_sdk_unload() };
    }
    if matches!(route(), Route::Vulkan) {
        unsafe { rsf_unity_package_sdk_unbind_vulkan() };
    }
    rsf_unity_frames_activate(0);
    unsafe { rsf_unity_package_sdk_unbind_d3d11() };
    RENDERER.store(RSF_UNITY_BRIDGE_NONE, SeqCst);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_unity_package_bridge_prepare(
    host: *const rsf_game_host_services,
    api: *mut rsf_unity_native_api,
) -> u32 {
    unsafe { prepare(host, api) }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_bridge_activate(enabled: u32) {
    rsf_unity_frames_activate(enabled);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_bridge_open(editor: u32) -> u32 {
    match route() {
        Route::Vulkan => unsafe {
            rsf_unity_vulkan_open();
            u32::from(editor != 0 || rsf_uvk_bridge_create_window())
        },
        Route::D3d11 => {
            d3d11_state::open();
            1
        }
        Route::D3d12 => 1,
        Route::None => 0,
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_bridge_try_close() -> u32 {
    rsf_unity_frames_activate(0);
    let route = route();
    let vulkan = matches!(route, Route::Vulkan);
    if vulkan {
        unsafe { rsf_unity_vulkan_stop_admission() };
    }
    {
        let _locked = host_lock();
        if rsf_unity_frames_try_close() == 0 {
            return 0;
        }
        let drained = match route {
            Route::D3d12 => rsf_unity_frames_drained(bridge_d3d12::completed()) != 0,
            Route::Vulkan => true,
            Route::D3d11 | Route::None => d3d11_state::try_close(),
        };
        if !drained {
            return 0;
        }
    }
    if vulkan && !unsafe { rsf_unity_vulkan_try_close() } {
        return 0;
    }
    if unsafe { rsf_unity_package_editor() == 0 || rsf_unity_preview_drained() } {
        return 1;
    }
    if vulkan {
        unsafe { rsf_unity_vulkan_finish_retirement(false) };
    }
    0
}

#[unsafe(no_mangle)]
extern "C" fn rsf_unity_package_bridge_finish_retirement(commit: u32) {
    let commit = commit != 0;
    let route = route();
    let vulkan = matches!(route, Route::Vulkan);
    if commit && unsafe { rsf_unity_package_editor() } != 0 {
        unsafe { rsf_unity_preview_release() };
    }
    if vulkan {
        unsafe { rsf_unity_vulkan_finish_retirement(commit) };
    }
    if !commit {
        return;
    }
    if matches!(route, Route::D3d12) {
        release_d3d12();
        rsf_unity_package_sr_slots_released(12);
        return;
    }
    let mut locked = host_lock();
    if !vulkan {
        if d3d11_state::release() {
            rsf_unity_package_sr_slots_released(11);
        } else if let Some(host) = locked.as_ref()
            && let Some(log) = host.0.log
        {
            unsafe {
                log(host.0.user, c"Unity DX11 bridge release refused without a checked close; its graphics remain owned.".as_ptr())
            };
        }
    }
    *locked = None;
    rsf_unity_frames_release();
}

pub(super) fn render(
    id: c_int,
    offset: u32,
    host: &rsf_game_host_services,
    packet: &rsf_unity_packet,
) {
    match route() {
        Route::Vulkan => {
            let mut change = 0;
            let reconfigure =
                offset == 0 && unsafe { rsf_unity_frames_pending_change(&mut change) } != 0;
            let delivered =
                unsafe { rsf_uvk_bridge_render(id, event_id(), packet, host, reconfigure) };
            if delivered && reconfigure {
                rsf_unity_frames_settle_change(change);
            }
        }
        Route::D3d11 if unsafe { rsf_unity_package_sdk_has_d3d11() } => {
            let editor = unsafe { rsf_unity_package_editor() } != 0;
            unsafe { d3d11::render_packet(host, offset, editor, packet) };
        }
        _ => {}
    }
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn graphics() -> Option<(ID3D12Device, ID3D12CommandQueue)> {
    match route() {
        Route::D3d12 => {
            let engine = unsafe { rsf_unity_d3d12_sdk_engine().as_ref() }?;
            let device = unsafe { engine.device.unwrap()() };
            let device = unsafe { ID3D12Device::from_raw_borrowed(&device) }.cloned();
            let queue = unsafe { engine.queue.unwrap()() };
            let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) }.cloned();
            device.zip(queue)
        }
        Route::D3d11 => {
            let _callback = Callback::enter()?;
            d3d11::graphics()
        }
        Route::Vulkan => {
            let (mut device, mut queue) = (null_mut(), null_mut());
            if !unsafe { rsf_uvk_bridge_graphics(&mut device, &mut queue) } {
                return None;
            }
            Some(unsafe {
                (
                    ID3D12Device::from_raw(device),
                    ID3D12CommandQueue::from_raw(queue),
                )
            })
        }
        Route::None => None,
    }
}
