// SPDX-License-Identifier: MIT
// The leaves both render_host entries call. Production compiles this module; each entry's unit
// tests supply fakes with the same names instead.
use super::render_host::{Adapter, ResourceDesc};
use crate::com_ref;
pub(super) use crate::d3d11_d3d12::{create_fence, fence_completed, queue_signal};
use crate::d3d12_fence::wait_fence;
use crate::d3d12_host::{borrow, device_of};
use crate::dlss_native12::{Registration, exports::rsf_dlss_native12_registration_borrow};
use crate::sr12;
use crate::sys::{rsf_dlss_setup, rsf_sharpening_options, rsf_sr12_setup};
use crate::sys::{rsf_render_host_call_native_interface, rsf_render_host_native_interface_fn};
use rsf_game_sys::rsf_game_render_pass;
use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::{
    Direct3D12::{
        D3D12_COMMAND_LIST_TYPE_DIRECT, ID3D12CommandQueue, ID3D12Device, ID3D12DeviceChild,
        ID3D12Resource,
    },
    Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_SWAP_CHAIN_DESC1, IDXGIAdapter1,
        IDXGIFactory4, IDXGISwapChain,
    },
};
use windows::core::Interface;
use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::{
        LibraryLoader::GetModuleHandleW,
        SystemInformation::GetTickCount64,
        Threading::{CreateEventW, GetCurrentProcessId},
    },
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
};
use windows_sys::core::w;

type Handle = *mut c_void;

pub(super) unsafe fn native_object(object: Handle) -> Handle {
    let module = unsafe { GetModuleHandleW(w!("sl.interposer.dll")) };
    let entry = unsafe {
        rsf_game_sys::win32::typed_export::<rsf_render_host_native_interface_fn>(
            module,
            c"slGetNativeInterface",
        )
    }
    .flatten();
    let mut native = null_mut();
    if let Some(entry) = entry
        && unsafe { rsf_render_host_call_native_interface(Some(entry), object, &mut native) } == 0
        && !native.is_null()
    {
        return native;
    }
    unsafe { com_ref::retain(object) }.into_raw()
}

pub(super) unsafe fn registration_borrow(
    device: Handle,
    setup: *const rsf_dlss_setup,
    out: *mut *mut Registration,
) -> i32 {
    let module = unsafe { GetModuleHandleW(w!("sl.interposer.dll")) };
    unsafe { rsf_dlss_native12_registration_borrow(device, module, setup, out) }
}

pub(super) unsafe fn tick() -> u64 {
    unsafe { GetTickCount64() }
}

pub(super) unsafe fn foreground() -> u32 {
    let mut owner = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), &mut owner);
        u32::from(owner == GetCurrentProcessId())
    }
}

pub(super) unsafe fn child_device(child: Handle) -> Handle {
    device_of(unsafe { borrow::<ID3D12DeviceChild>(&child) })
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn adapter(device: Handle, out: *mut Adapter) -> i32 {
    let device = unsafe { borrow::<ID3D12Device>(&device) };
    let Ok(desc) = (unsafe {
        CreateDXGIFactory1::<IDXGIFactory4>()
            .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter1>(device.GetAdapterLuid()))
            .and_then(|adapter| adapter.GetDesc1())
    }) else {
        return 0;
    };
    unsafe {
        out.write(Adapter {
            vendor: desc.VendorId,
            device: desc.DeviceId,
            software: u32::from(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0),
        });
    }
    1
}

pub(super) unsafe fn last_present_count(chain: Handle, out: *mut u32) -> i32 {
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return 0;
    };
    let Ok(count) = (unsafe { chain.GetLastPresentCount() }) else {
        return 0;
    };
    unsafe { out.write(count) };
    1
}

pub(super) unsafe fn resource_desc(resource: Handle, out: *mut ResourceDesc) {
    let desc = unsafe { borrow::<ID3D12Resource>(&resource).GetDesc() };
    unsafe {
        out.write(ResourceDesc {
            width: desc.Width,
            height: desc.Height,
            format: desc.Format.0 as u32,
            samples: desc.SampleDesc.Count,
        });
    }
}

unsafe extern "C" {
    // The presentation bridge's explicit creation (d3d11_present_bridge.h), a C leaf this crate's
    // build links. The explicit render host calls it at the engine's creation site.
    fn rsf_d3d11_present_create_for_hwnd(
        factory: Handle,
        queue: Handle,
        window: Handle,
        desc: *const DXGI_SWAP_CHAIN_DESC1,
        out: *mut Handle,
    ) -> i32;
}

pub(super) unsafe fn present_create_for_hwnd(
    factory: Handle,
    queue: Handle,
    window: Handle,
    description: *const c_void,
    out: *mut Handle,
) -> i32 {
    unsafe { rsf_d3d11_present_create_for_hwnd(factory, queue, window, description.cast(), out) }
}

pub(super) unsafe fn queue_direct(queue: Handle) -> i32 {
    let desc = unsafe { borrow::<ID3D12CommandQueue>(&queue).GetDesc() };
    i32::from(desc.Type == D3D12_COMMAND_LIST_TYPE_DIRECT)
}

pub(super) unsafe fn fence_wait(fence: Handle, value: u64, timeout: u32) -> i32 {
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return 0;
    }
    let status = unsafe { wait_fence(fence, value, event, timeout) };
    unsafe { CloseHandle(event) };
    if status >= 0 { 1 } else { -1 }
}

pub(super) use crate::d3d11_present::{
    rsf_d3d11_present_backend as present_backend,
    rsf_d3d11_present_backend_choices as present_backend_choices,
    rsf_d3d11_present_has_owner as present_has_owner, rsf_d3d11_present_host as present_host,
    rsf_d3d11_present_request as present_request,
    rsf_d3d11_present_switch_result as present_switch_result,
};
pub(super) use crate::d3d12_observer::attachment::{
    rsf_d3d12_observer_device as observer_device, rsf_d3d12_observer_factory as observer_factory,
};
pub(super) use crate::d3d12_observer::diagnostics::rsf_d3d12_diagnostics_enable as diagnostics_enable;
pub(super) use crate::d3d12_observer::rsf_d3d12_observer_create as observer_create;
pub(super) use crate::dlss_native12::exports::{
    rsf_dlss_native12_registration_create as registration_create,
    rsf_dlss_native12_registration_present as registration_present,
};
pub(super) use crate::fg_choice::{
    rsf_fg_choice_save as fg_choice_save, rsf_fg_default_backend as fg_default_backend,
};
pub(super) use crate::fg12::{
    rsf_core_fg12_capture_outputs as fg_capture_outputs, rsf_core_fg12_cpu as fg_cpu,
    rsf_core_fg12_hudless as fg_hudless, rsf_core_fg12_install as fg_install,
    rsf_core_fg12_install_explicit as fg_install_explicit, rsf_core_fg12_latency as fg_latency,
    rsf_core_fg12_options as fg_options, rsf_core_fg12_status as fg_status,
    rsf_core_fg12_status_try as fg_status_try, rsf_core_fg12_submitted as fg_submitted,
    rsf_core_fg12_window as fg_window,
};
pub(super) use crate::input_dump::abi::rsf_input_dump_cancel as input_dump_cancel;
pub(super) use crate::overlay_native12::rsf_overlay_native12_present as overlay_present;
pub(super) use crate::overlay_settings::rsf_overlay_fill_fg_stats as overlay_fill_fg;
pub(super) use crate::overlay_wrapped12::{
    rsf_overlay_d3d12_frame as overlay_frame, rsf_overlay_d3d12_stop as overlay_stop,
};
pub(super) use crate::plugin_session::{
    rsf_plugin_session_prepare as session_prepare, rsf_plugin_session_quiesce as session_quiesce,
    rsf_plugin_session_start as session_start, rsf_plugin_session_stop as session_stop,
};

pub(super) unsafe fn depth_domains(pass: *const rsf_game_render_pass) -> u32 {
    unsafe {
        u32::from(rsf_game_sys::depth_domains_requested(
            &(*pass).depth_domains,
        ))
    }
}

pub(super) fn dlss_preset_valid(preset: u32) -> u32 {
    u32::from(crate::streamline_sr::preset_valid(preset))
}

// The hosts hold SR contexts as untyped handles.
pub(super) unsafe fn sr12_create(setup: *const rsf_sr12_setup, out: *mut Handle) -> i32 {
    unsafe { sr12::rsf_core_sr12_create(setup, out.cast()) }
}

pub(super) unsafe fn sr12_plan(context: Handle, width: *mut u32, height: *mut u32) -> i32 {
    unsafe { sr12::rsf_core_sr12_plan(context.cast(), width, height) }
}

pub(super) unsafe fn sr12_destroy(context: Handle) {
    unsafe { sr12::rsf_core_sr12_destroy(context.cast()) }
}

pub(super) unsafe fn sr12_evaluate(
    context: Handle,
    commands: Handle,
    pass: *const rsf_game_render_pass,
    slot: u32,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_evaluate(context.cast(), commands, pass, slot) }
}

pub(super) unsafe fn sr12_set_sharpening(
    context: Handle,
    options: *const rsf_sharpening_options,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_set_sharpening(context.cast(), options) }
}

pub(super) unsafe fn sr12_present(
    context: Handle,
    chain: Handle,
    interval: u32,
    flags: u32,
    begin: u32,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_present(context.cast(), chain, interval, flags, begin) }
}

pub(super) unsafe fn sr12_capture(
    context: Handle,
    path: *const c_char,
    first_frame: u64,
    frames: u32,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_capture(context.cast(), path, first_frame, frames) }
}

pub(super) unsafe fn sr12_capture_flush(context: Handle, slot: u32) -> i32 {
    unsafe { sr12::rsf_core_sr12_capture_flush(context.cast(), slot) }
}

pub(super) unsafe fn sr12_capture_scene(
    context: Handle,
    commands: Handle,
    pass: *const rsf_game_render_pass,
    slot: u32,
) -> i32 {
    unsafe { sr12::rsf_core_sr12_capture_scene(context.cast(), commands, pass, slot) }
}
