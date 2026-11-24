// SPDX-License-Identifier: MIT
//! Device admission and the three deployed motion fixtures. The C11 launchers in tests/ call
//! rsf_anomaly_motion_device_fixture, rsf_anomaly_motion_far_device_fixture and
//! rsf_anomaly_motion_tess_chain_device_fixture.
mod core_device;
mod draw;
mod far_device;
mod foliage;
mod harness;
mod model;
mod scene;
mod schedule;
mod tess_chain_device;
use harness::Gpu;
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

/// A hardware D3D11.0 device, NVIDIA first, with the debug layer when `try_debug` and it is installed.
/// Returns the device and whether the debug layer is on.
fn hardware(try_debug: bool) -> Option<(Gpu, bool)> {
    let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
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
                let gpu = Gpu {
                    device: candidate,
                    context: immediate,
                };
                return Some((gpu, debug));
            }
        }
    }
    None
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
    let adapter = adapter.map(<&IDXGIAdapter>::from);
    let (device, context) =
        rsf_test_support::d3d11_device_on(adapter, driver, flags, levels).ok()?;
    let level = unsafe { device.GetFeatureLevel() };
    Some((device, context, level))
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

/// The NVIDIA adapter (else the default hardware adapter, else WARP), debug layer first, any feature level.
fn default_device() -> Option<(Gpu, D3D_FEATURE_LEVEL)> {
    let adapter = first_nvidia();
    let driver = if adapter.is_some() {
        D3D_DRIVER_TYPE_UNKNOWN
    } else {
        D3D_DRIVER_TYPE_HARDWARE
    };
    let (result, debug, warp) = match create_device(adapter.as_ref(), driver, true, None) {
        Some(result) => (result, true, false),
        None => match create_device(adapter.as_ref(), driver, false, None) {
            Some(result) => (result, false, false),
            None => (
                create_device(None, D3D_DRIVER_TYPE_WARP, false, None)?,
                false,
                true,
            ),
        },
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
    let gpu = Gpu {
        device: candidate,
        context: immediate,
    };
    Some((gpu, level))
}

fn adapter_name(description: &[u16]) -> String {
    let end = description
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(description.len());
    String::from_utf16_lossy(&description[..end])
}
