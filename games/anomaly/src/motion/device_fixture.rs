// SPDX-License-Identifier: MIT
//! Device admission and scenario scheduling for the deployed motion fixtures.
mod draw;
mod foliage;
mod model;
mod schedule;
use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_UNKNOWN, D3D_DRIVER_TYPE_WARP,
    D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter, IDXGIAdapter1, IDXGIDevice,
    IDXGIFactory1,
};
use windows::core::Interface;

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_fixture_hardware(
    try_debug: bool,
    device: &mut *mut c_void,
    context: &mut *mut c_void,
    debug_layer: &mut bool,
) -> bool {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return false;
    };
    // Preserve NVIDIA preference across both debug and non-debug attempts.
    for vendor in [Some(0x10de), None] {
        for debug in [true, false] {
            if debug && !try_debug {
                continue;
            }
            for index in 0.. {
                let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                    break;
                };
                let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                    continue;
                };
                if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
                    || vendor.is_some_and(|id| desc.VendorId != id)
                {
                    continue;
                }
                let Some((candidate, immediate, _)) = create_device(
                    Some(&adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    debug,
                    Some(&[D3D_FEATURE_LEVEL_11_0]),
                ) else {
                    continue;
                };
                let name = adapter_name(&desc.Description);
                let debug_name = if debug { "on" } else { "off" };
                let fallback = if vendor.is_none() {
                    " -- first available hardware fallback"
                } else {
                    ""
                };
                println!(
                    "device: {name} (vendor {:#06x}, debug layer {debug_name}){fallback}",
                    desc.VendorId
                );
                *device = candidate.into_raw();
                *context = immediate.into_raw();
                *debug_layer = debug;
                return true;
            }
        }
    }
    false
}

fn create_device(
    adapter: Option<&IDXGIAdapter1>,
    driver: D3D_DRIVER_TYPE,
    debug: bool,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Option<(ID3D11Device, ID3D11DeviceContext, D3D_FEATURE_LEVEL)> {
    let flags = if debug {
        D3D11_CREATE_DEVICE_DEBUG
    } else {
        D3D11_CREATE_DEVICE_FLAG(0)
    };
    let (mut device, mut context) = (None, None);
    let mut level = D3D_FEATURE_LEVEL::default();
    let adapter = adapter.map(<&IDXGIAdapter>::from);
    unsafe {
        D3D11CreateDevice(
            adapter,
            driver,
            Default::default(),
            flags,
            levels,
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut level),
            Some(&mut context),
        )
    }
    .ok()?;
    Some((device?, context?, level))
}

fn first_nvidia() -> Option<IDXGIAdapter1> {
    let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
    (0..)
        .map_while(|i| unsafe { factory.EnumAdapters1(i) }.ok())
        .find(|adapter| {
            unsafe { adapter.GetDesc1() }.is_ok_and(|desc| {
                desc.VendorId == 0x10de && desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
            })
        })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_motion_fixture_default(
    device: &mut *mut c_void,
    context: &mut *mut c_void,
    feature_level: &mut i32,
) -> bool {
    let adapter = first_nvidia();
    let driver = if adapter.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    let (result, debug, warp) =
        if let Some(result) = create_device(adapter.as_ref(), driver, true, None) {
            (result, true, false)
        } else if let Some(result) = create_device(adapter.as_ref(), driver, false, None) {
            (result, false, false)
        } else if let Some(result) = create_device(None, D3D_DRIVER_TYPE_WARP, false, None) {
            (result, false, true)
        } else {
            return false;
        };
    let (candidate, immediate, level) = result;
    let desc = candidate
        .cast::<IDXGIDevice>()
        .ok()
        .and_then(|dxgi| unsafe { dxgi.GetAdapter().ok()?.GetDesc().ok() });
    let name = desc.map_or_else(String::new, |desc| adapter_name(&desc.Description));
    let driver_name = if warp { "WARP" } else { "hardware" };
    let debug_name = if debug { "on" } else { "off" };
    println!("device: {name} ({driver_name}, debug layer {debug_name})");
    *device = candidate.into_raw();
    *context = immediate.into_raw();
    *feature_level = level.0;
    true
}

fn adapter_name(description: &[u16]) -> String {
    let end = description
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(description.len());
    String::from_utf16_lossy(&description[..end])
}
