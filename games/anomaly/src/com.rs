// SPDX-License-Identifier: MIT
//! Raw COM reference counting for the D3D11 handles the owners hold as `Lease`s.
use std::ffi::c_void;
use windows::core::{IUnknown, Interface};

/// Adds one reference to a live COM object.
pub(crate) unsafe extern "C" fn retain(raw: *mut c_void) {
    let object = unsafe { IUnknown::from_raw_borrowed(&raw) }.unwrap();
    unsafe { (object.vtable().AddRef)(raw) };
}

/// Releases one reference; null is ignored.
pub(crate) unsafe extern "C" fn release(raw: *mut c_void) {
    if !raw.is_null() {
        drop(unsafe { IUnknown::from_raw(raw) });
    }
}
