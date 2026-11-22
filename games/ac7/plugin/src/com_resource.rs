// SPDX-License-Identifier: MIT
use std::ffi::c_void;
use windows::core::{IUnknown, Interface};

pub(crate) unsafe extern "C" fn retain(resource: *mut c_void) {
    if let Some(resource) = unsafe { IUnknown::from_raw_borrowed(&resource) } {
        unsafe { (resource.vtable().AddRef)(resource.as_raw()) };
    }
}

pub(crate) unsafe extern "C" fn release(resource: *mut c_void) {
    if !resource.is_null() {
        drop(unsafe { IUnknown::from_raw(resource) });
    }
}
