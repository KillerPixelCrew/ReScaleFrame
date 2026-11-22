// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D11::ID3D11DeviceChild;
use windows::core::{IUnknown, Interface};

pub(super) unsafe fn identity(object: *mut c_void) -> Option<IUnknown> {
    unsafe { IUnknown::from_raw_borrowed(&object) }?.cast().ok()
}

pub(super) unsafe fn owns(device: *mut c_void, child: *mut c_void) -> bool {
    if device.is_null() {
        return false;
    }
    let Some(child) = (unsafe { ID3D11DeviceChild::from_raw_borrowed(&child) }) else {
        return false;
    };
    let Ok(owner) = (unsafe { child.GetDevice() }) else {
        return false;
    };
    if owner.as_raw() == device {
        return true;
    }
    let expected = unsafe { identity(device) };
    let actual = owner.cast::<IUnknown>();
    matches!((expected, actual), (Some(expected), Ok(actual)) if expected.as_raw() == actual.as_raw())
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_device_owns(device: *mut c_void, child: *mut c_void) -> i32 {
    i32::from(unsafe { owns(device, child) })
}
