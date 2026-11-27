// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D11::ID3D11Device;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{IUnknown, Interface};

pub(super) unsafe fn identity(object: *mut c_void) -> Option<IUnknown> {
    unsafe { IUnknown::from_raw_borrowed(&object) }?.cast().ok()
}

pub(super) unsafe fn device_vendor(device: *mut c_void) -> Option<u32> {
    let device = unsafe { ID3D11Device::from_raw_borrowed(&device) }?;
    let dxgi = device.cast::<IDXGIDevice>().ok()?;
    let desc = unsafe { dxgi.GetAdapter().ok()?.GetDesc() }.ok()?;
    Some(desc.VendorId)
}
