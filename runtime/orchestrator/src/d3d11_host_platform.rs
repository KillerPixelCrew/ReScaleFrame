// SPDX-License-Identifier: MIT
//! Typed Windows calls for the host's production platform table.
use crate::d3d11_device_identity::identity;
use std::ffi::c_void;
use std::ptr::null_mut;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIDeviceSubObject, IDXGISwapChain};
use windows::Win32::System::WindowsProgramming::{
    GetPrivateProfileIntW, WritePrivateProfileStringW,
};
use windows::core::{IUnknown, Interface, PCWSTR};

type Raw = *mut c_void;

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_same_object(left: Raw, right: Raw) -> i32 {
    if right.is_null() {
        return 0;
    }
    let Some(left) = (unsafe { identity(left) }) else {
        return 0;
    };
    i32::from(unsafe { identity(right) }.is_some_and(|right| left.as_raw() == right.as_raw()))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_immediate_context(device: Raw) -> Raw {
    unsafe { ID3D11Device::from_raw_borrowed(&device) }
        .and_then(|device| unsafe { device.GetImmediateContext() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

pub(super) unsafe fn device_vendor(device: Raw) -> Option<u32> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let dxgi = device.cast::<IDXGIDevice>().ok()?;
    let desc = unsafe { dxgi.GetAdapter()?.GetDesc() }.ok()?;
    Some(desc.VendorId)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_device_vendor(device: Raw, vendor: *mut u32) -> i32 {
    let Some(id) = (unsafe { device_vendor(device) }) else {
        return 0;
    };
    unsafe { vendor.write(id) };
    1
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_device(chain: Raw) -> Raw {
    unsafe { IDXGIDeviceSubObject::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDevice::<ID3D11Device>() }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_window(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetDesc() }.ok())
        .map_or(null_mut(), |desc| desc.OutputWindow.0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_buffer(chain: Raw) -> Raw {
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .and_then(|chain| unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) }.ok())
        .map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_com_identity(object: Raw) -> Raw {
    unsafe { identity(object) }.map_or(null_mut(), Interface::into_raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_chain_present(chain: Raw) -> Raw {
    // Log the entry from this receiver's own vtable, without dispatching it.
    unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }
        .map_or(null_mut(), |chain| chain.vtable().Present as Raw)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_com_release(object: Raw) -> u32 {
    let Some(object) = (unsafe { IUnknown::from_raw_borrowed(&object) }) else {
        return 0;
    };
    unsafe { (object.vtable().Release)(object.as_raw()) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_ini_read(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    fallback: i32,
) -> i32 {
    unsafe { GetPrivateProfileIntW(PCWSTR(section), PCWSTR(key), fallback, PCWSTR(path)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11h_ini_write(
    path: *const u16,
    section: *const u16,
    key: *const u16,
    value: *const u16,
) -> i32 {
    i32::from(unsafe {
        WritePrivateProfileStringW(PCWSTR(section), PCWSTR(key), PCWSTR(value), PCWSTR(path))
            .is_ok()
    })
}
