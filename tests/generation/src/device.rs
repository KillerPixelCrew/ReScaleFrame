// SPDX-License-Identifier: MIT
use crate::session::{NOT_SUPPORTED, Raw};
use crate::sys::*;
use std::ffi::{c_char, c_int, c_ulong};
use std::ptr::null_mut;
use windows::Win32::{
    Foundation::{CloseHandle, HWND, WAIT_OBJECT_0},
    Graphics::{
        Direct3D12::*,
        Dxgi::{Common::DXGI_FORMAT_R8G8B8A8_UNORM, *},
    },
    System::Threading::{CreateEventW, WaitForSingleObject},
};
use windows::core::{Interface, PCWSTR, Result};

pub(super) const NOT_COMPILED: i32 = -3;
pub(super) const SKIP: i32 = 77;
pub(super) const WIDTH: u32 = 1920;
pub(super) const HEIGHT: u32 = 1080;

unsafe extern "C" {
    pub(super) fn rsf_fg_get_provider(backend: u32) -> *const rsf_generation_provider;
    fn strtoul(input: *const c_char, end: *mut *mut c_char, radix: c_int) -> c_ulong;
}

pub(super) unsafe fn numeric_arg(arg: *const c_char) -> u32 {
    // The old fixture accepted the Windows CRT's base-0 syntax, including hex.
    unsafe { strtoul(arg, null_mut(), 0) as u32 }
}

pub(super) fn unavailable(result: i32) -> bool {
    matches!(result, NOT_COMPILED | NOT_SUPPORTED)
}

pub(super) fn factory() -> Result<IDXGIFactory4> {
    unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
}

pub(super) fn adapter(factory: &IDXGIFactory4, vendor: u32) -> Result<Option<IDXGIAdapter1>> {
    for index in 0..=u32::MAX {
        let candidate: IDXGIAdapter1 = match unsafe { factory.EnumAdapters1(index) } {
            Ok(candidate) => candidate,
            Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => return Ok(None),
            Err(error) => return Err(error),
        };
        let desc = unsafe { candidate.GetDesc1()? };
        if desc.VendorId == vendor && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0 {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

pub(super) fn device_and_queue(
    adapter: &IDXGIAdapter1,
) -> Result<(ID3D12Device, ID3D12CommandQueue)> {
    let device = rsf_test_support::d3d12_device(adapter)?;
    let queue = unsafe {
        device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC {
            Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
            ..Default::default()
        })?
    };
    Ok((device, queue))
}

pub(super) use rsf_test_support::Window;

pub(super) fn drain(queue: &ID3D12CommandQueue, fence: &ID3D12Fence, value: u64) -> bool {
    let Ok(event) = (unsafe { CreateEventW(None, false, false, PCWSTR::null()) }) else {
        return false;
    };
    unsafe {
        let complete = queue.Signal(fence, value).is_ok()
            && fence.SetEventOnCompletion(value, event).is_ok()
            && WaitForSingleObject(event, 15_000) == WAIT_OBJECT_0;
        let _ = CloseHandle(event);
        complete
    }
}

pub(super) fn setup(window: HWND, device: Raw, queue: Raw, generated: u32) -> rsf_generation_setup {
    rsf_generation_setup {
        struct_size: size_of::<rsf_generation_setup>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        chain: rsf_fg_swapchain_desc {
            struct_size: size_of::<rsf_fg_swapchain_desc>() as u32,
            abi_version: RSF_BACKEND_ABI_VERSION,
            hwnd: window.0,
            d3d12_device: device,
            d3d12_queue: queue,
            width: WIDTH,
            height: HEIGHT,
            format: DXGI_FORMAT_R8G8B8A8_UNORM.0 as u32,
            buffer_count: 3,
            max_generated_frames: generated,
            ui_mode: RSF_UI_MODE_NONE,
            ..unsafe { std::mem::zeroed() }
        },
        depth_inverted: 1,
        depth_infinite: 1,
        view_space_to_meters: 1.0,
        ..unsafe { std::mem::zeroed() }
    }
}

pub(super) fn disabled_options() -> rsf_fg_options {
    rsf_fg_options {
        struct_size: size_of::<rsf_fg_options>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        mode: RSF_FG_OFF,
        generated_frames: 1,
        dynamic_target_fps: 0.0,
        reflex_mode: RSF_REFLEX_OFF,
        frame_limit_us: 0,
    }
}

pub(super) unsafe fn disabled_present(chain: Raw) -> Option<i32> {
    let swap = unsafe { IDXGISwapChain4::from_raw_borrowed(&chain) }?;
    // Keep the real backbuffer reference through the disabled Present, then release it.
    let _backbuffer =
        unsafe { swap.GetBuffer::<ID3D12Resource>(swap.GetCurrentBackBufferIndex()) }.ok()?;
    Some(unsafe { swap.Present(0, DXGI_PRESENT(0)) }.0)
}
