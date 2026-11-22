// SPDX-License-Identifier: MIT
//! Package renderer routing and bridge retirement.
use super::*;
use crate::d3d11_state::{self, graphics as d3d11};
use crate::package_state::{rsf_unity_package_sr_slots_released, services::editor};
use crate::sys::*;
use crate::{preview, vulkan_state};
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

pub(crate) unsafe extern "C" fn rsf_unity_package_bridge_load(interfaces: *mut c_void) -> u32 {
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

pub(crate) extern "C" fn rsf_unity_package_bridge_unload() {
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

pub(crate) unsafe extern "C" fn rsf_unity_package_bridge_prepare(
    host: *const rsf_game_host_services,
    api: *mut rsf_unity_native_api,
) -> u32 {
    unsafe { prepare(host, api) }
}

pub(crate) extern "C" fn rsf_unity_package_bridge_open(editor: u32) -> u32 {
    match route() {
        Route::Vulkan => {
            vulkan_state::open();
            u32::from(editor != 0 || vulkan_state::bridge::create_window())
        }
        Route::D3d11 => {
            d3d11_state::open();
            1
        }
        Route::D3d12 => 1,
        Route::None => 0,
    }
}

pub(crate) extern "C" fn rsf_unity_package_bridge_try_close() -> u32 {
    rsf_unity_frames_activate(0);
    let route = route();
    let vulkan = matches!(route, Route::Vulkan);
    if vulkan {
        vulkan_state::stop_admission();
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
    if vulkan && !vulkan_state::try_close() {
        return 0;
    }
    if !editor() || preview::rsf_unity_preview_drained() {
        return 1;
    }
    if vulkan {
        vulkan_state::finish_retirement(false);
    }
    0
}

pub(crate) extern "C" fn rsf_unity_package_bridge_finish_retirement(commit: u32) {
    let commit = commit != 0;
    let route = route();
    let vulkan = matches!(route, Route::Vulkan);
    if commit && editor() {
        preview::rsf_unity_preview_release();
    }
    if vulkan {
        vulkan_state::finish_retirement(commit);
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
                rsf_game_sys::rsf_game_log_guarded(Some(log), host.0.user, c"Unity DX11 bridge release refused without a checked close; its graphics remain owned.".as_ptr())
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
                vulkan_state::bridge::render_event(id, event_id(), packet, host, reconfigure);
            if delivered && reconfigure {
                rsf_unity_frames_settle_change(change);
            }
        }
        Route::D3d11 if unsafe { rsf_unity_package_sdk_has_d3d11() } => {
            unsafe { d3d11::render_packet(host, offset, editor(), packet) };
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
        Route::Vulkan => vulkan_state::bridge::graphics(),
        Route::None => None,
    }
}
