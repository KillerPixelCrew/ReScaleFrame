// SPDX-License-Identifier: MIT
//! Real hardware and WARP device creation for the SR fixtures.
use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::Interface;

pub(crate) struct Device {
    pub(crate) device: ID3D11Device,
    pub(crate) context: ID3D11DeviceContext,
    pub(crate) info: Option<ID3D11InfoQueue>,
    pub(crate) level: D3D_FEATURE_LEVEL,
}

impl Device {
    fn create(
        adapter: Option<&IDXGIAdapter>,
        driver: D3D_DRIVER_TYPE,
        flags: D3D11_CREATE_DEVICE_FLAG,
        levels: Option<&[D3D_FEATURE_LEVEL]>,
    ) -> Option<Self> {
        let (mut device, mut context, mut level) = (None, None, D3D_FEATURE_LEVEL::default());
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
        let device: ID3D11Device = device?;
        let info = if flags == D3D11_CREATE_DEVICE_DEBUG {
            device.cast().ok()
        } else {
            None
        };
        Some(Self {
            device,
            context: context?,
            level,
            info,
        })
    }

    pub(crate) fn hardware_or_warp(roundtrip: bool) -> Option<Self> {
        let levels = [D3D_FEATURE_LEVEL_11_0];
        let levels = roundtrip.then_some(levels.as_slice());
        let mut attempts = vec![
            (
                D3D_DRIVER_TYPE_HARDWARE,
                D3D11_CREATE_DEVICE_DEBUG,
                "hardware",
            ),
            (
                D3D_DRIVER_TYPE_HARDWARE,
                D3D11_CREATE_DEVICE_FLAG(0),
                "hardware",
            ),
        ];
        if roundtrip {
            attempts.push((D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_DEBUG, "WARP"));
        }
        attempts.push((D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_FLAG(0), "WARP"));
        for (driver, flags, kind) in attempts {
            if let Some(device) = Self::create(None, driver, flags, levels) {
                let name = device
                    .device
                    .cast::<IDXGIDevice>()
                    .and_then(|dxgi| unsafe { dxgi.GetAdapter() })
                    .and_then(|adapter| unsafe { adapter.GetDesc() })
                    .map(|desc| wide_name(&desc.Description))
                    .unwrap_or_default();
                let debug = if flags == D3D11_CREATE_DEVICE_DEBUG {
                    "on"
                } else {
                    "off"
                };
                println!("device: {name} ({kind}, debug layer {debug})");
                return Some(device);
            }
        }
        None
    }

    fn hardware(vendor: u32, debug: bool) -> Option<Self> {
        let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }.ok()?;
        let levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        for index in 0.. {
            let adapter = unsafe { factory.EnumAdapters1(index) }.ok()?;
            let desc = unsafe { adapter.GetDesc1() }.ok()?;
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0
                || (vendor != 0 && desc.VendorId != vendor)
            {
                continue;
            }
            let adapter = adapter.cast().ok()?;
            let plain = D3D11_CREATE_DEVICE_FLAG(0);
            let preferred = if debug {
                D3D11_CREATE_DEVICE_DEBUG
            } else {
                plain
            };
            for flags in [preferred, plain] {
                if let Some(device) = Self::create(
                    Some(&adapter),
                    D3D_DRIVER_TYPE_UNKNOWN,
                    flags,
                    Some(&levels),
                ) {
                    let name = wide_name(&desc.Description);
                    let debug = if flags == D3D11_CREATE_DEVICE_DEBUG {
                        "on"
                    } else {
                        "off"
                    };
                    println!(
                        "device: {name} (vendor 0x{:04x}, debug layer {debug})",
                        desc.VendorId
                    );
                    return Some(device);
                }
            }
        }
        None
    }
}

fn wide_name(description: &[u16]) -> String {
    String::from_utf16_lossy(description)
        .trim_end_matches('\0')
        .to_owned()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_sr_fixture_device(
    hardware_only: bool,
    vendor: u32,
    debug: bool,
    device: &mut *mut c_void,
    context: &mut *mut c_void,
    info: &mut *mut c_void,
) -> bool {
    let result = if hardware_only {
        Device::hardware(vendor, debug)
    } else {
        Device::hardware_or_warp(true)
    };
    let Some(result) = result else { return false };
    *device = result.device.into_raw();
    *context = result.context.into_raw();
    *info = result
        .info
        .map_or(std::ptr::null_mut(), Interface::into_raw);
    true
}
