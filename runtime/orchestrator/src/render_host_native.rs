// SPDX-License-Identifier: MIT
use super::render_host_methods::{Adapter, ResourceDesc};
use crate::com_ref::ComRef;
use crate::d3d12_host::{borrow, device_of, rsf_core_d3d12_wait_native_fence};
use crate::dlss_native12::{Registration, exports::rsf_dlss_native12_registration_borrow};
use crate::sys::rsf_dlss_setup;
use crate::sys::{rsf_render_host_call_native_interface, rsf_render_host_native_interface_fn};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::{
    Direct3D12::{
        D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_FENCE_FLAG_NONE, ID3D12CommandQueue, ID3D12Device,
        ID3D12DeviceChild, ID3D12Fence, ID3D12Resource,
    },
    Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory4,
        IDXGISwapChain,
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
    unsafe { ComRef::retain(object) }.into_raw()
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

pub(super) unsafe fn queue_direct(queue: Handle) -> i32 {
    let desc = unsafe { borrow::<ID3D12CommandQueue>(&queue).GetDesc() };
    i32::from(desc.Type == D3D12_COMMAND_LIST_TYPE_DIRECT)
}

pub(super) unsafe fn create_fence(device: Handle) -> Handle {
    unsafe { borrow::<ID3D12Device>(&device).CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn fence_completed(fence: Handle) -> u64 {
    unsafe { borrow::<ID3D12Fence>(&fence).GetCompletedValue() }
}

pub(super) unsafe fn fence_wait(fence: Handle, value: u64, timeout: u32) -> i32 {
    let event = unsafe { CreateEventW(null(), 0, 0, null()) };
    if event.is_null() {
        return 0;
    }
    let status = unsafe { rsf_core_d3d12_wait_native_fence(fence, value, event, timeout) };
    unsafe { CloseHandle(event) };
    if status >= 0 { 1 } else { -1 }
}

pub(super) unsafe fn queue_signal(queue: Handle, fence: Handle, value: u64) -> i32 {
    i32::from(
        unsafe {
            borrow::<ID3D12CommandQueue>(&queue).Signal(borrow::<ID3D12Fence>(&fence), value)
        }
        .is_ok(),
    )
}
