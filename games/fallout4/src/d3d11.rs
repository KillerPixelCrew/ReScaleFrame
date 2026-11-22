// SPDX-License-Identifier: MIT

use std::ffi::c_void;
use windows::Win32::Graphics::Direct3D11::{D3D11_VIEWPORT, ID3D11DeviceContext};
use windows::core::{IUnknown, Interface};

pub(crate) fn identity<T: Interface>(object: &T) -> u64 {
    object
        .cast::<IUnknown>()
        .map_or(0, |identity| identity.as_raw() as u64)
}

pub(crate) fn same_native(object: u64, native: u64) -> bool {
    if object == 0 || native == 0 {
        return false;
    }
    let raw = native as *mut c_void;
    // SAFETY: native names an interface on the current renderer or a leased target.
    unsafe { IUnknown::from_raw_borrowed(&raw) }.is_some_and(|native| identity(native) == object)
}

pub(crate) fn viewport(context: &ID3D11DeviceContext) -> (u32, u32, [f32; 6]) {
    let mut count = 0;
    // SAFETY: the typed context is live. Request the count before reading a single viewport.
    unsafe { context.RSGetViewports(&mut count, None) };
    if count != 1 {
        return (count, 0, [0.0; 6]);
    }
    let mut viewport = D3D11_VIEWPORT::default();
    let mut copied = 1;
    unsafe { context.RSGetViewports(&mut copied, Some(&mut viewport)) };
    (
        count,
        copied,
        [
            viewport.TopLeftX,
            viewport.TopLeftY,
            viewport.Width,
            viewport.Height,
            viewport.MinDepth,
            viewport.MaxDepth,
        ],
    )
}
