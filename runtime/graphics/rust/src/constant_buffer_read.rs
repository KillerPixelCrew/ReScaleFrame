// SPDX-License-Identifier: MIT
use crate::com_ref::{self, vtable};
use crate::d3d11_output::same_device;
use crate::d3d11_output_platform::rsf_d3d11_output_is_immediate;
use std::ffi::c_void;
use std::ptr;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BUFFER_DESC, D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RESOURCE_DIMENSION, D3D11_RESOURCE_DIMENSION_BUFFER, D3D11_USAGE_STAGING, ID3D11Buffer,
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
};
use windows::core::{IUnknown, Interface};

type Handle = *mut c_void;

const OK: i32 = 0;
const INVALID: i32 = -1;
const TOO_SMALL: i32 = -3;
const STAGING_FAILED: i32 = -4;
const MAP_FAILED: i32 = -5;
const FOREIGN_DEVICE: i32 = -6;

unsafe fn create_buffer(device: Handle, desc: &D3D11_BUFFER_DESC) -> Option<IUnknown> {
    let mut buffer = ptr::null_mut();
    let status = unsafe {
        (vtable::<ID3D11Device>(&device).CreateBuffer)(device, desc, ptr::null(), &mut buffer)
    };
    unsafe { com_ref::from_result(status, buffer) }
}

/// # Safety
///
/// `buffer` must be a live `ID3D11Resource`.
pub unsafe fn resource_buffer_info(buffer: Handle) -> Option<(u32, u32, u32)> {
    let mut dimension = D3D11_RESOURCE_DIMENSION::default();
    unsafe { (vtable::<ID3D11Resource>(&buffer).GetType)(buffer, &mut dimension) };
    if dimension != D3D11_RESOURCE_DIMENSION_BUFFER {
        return None;
    }
    let mut desc = D3D11_BUFFER_DESC::default();
    unsafe { (vtable::<ID3D11Buffer>(&buffer).GetDesc)(buffer, &mut desc) };
    Some((desc.ByteWidth, desc.BindFlags, desc.Usage.0 as u32))
}

/// # Safety
///
/// Each handle is null or a live object of its role: `device` and its `context`, a constant
/// `buffer` of that device, and `destination` writable for `bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_read_constant_buffer(
    device: Handle,
    context: Handle,
    buffer: Handle,
    destination: Handle,
    bytes: u32,
) -> i32 {
    if device.is_null()
        || context.is_null()
        || buffer.is_null()
        || destination.is_null()
        || bytes == 0
    {
        return INVALID;
    }
    let Some((width, _, _)) = (unsafe { resource_buffer_info(buffer) }) else {
        return INVALID;
    };
    if !unsafe { same_device(device, buffer) && same_device(device, context) } {
        return FOREIGN_DEVICE;
    }
    if width < bytes {
        return TOO_SMALL;
    }
    // Avoid recording a deferred copy that cannot be followed by a read map.
    if unsafe { rsf_d3d11_output_is_immediate(context) } == 0 {
        return MAP_FAILED;
    }
    let Some(staging_buffer) = (unsafe {
        create_buffer(
            device,
            &D3D11_BUFFER_DESC {
                ByteWidth: width,
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..D3D11_BUFFER_DESC::default()
            },
        )
    }) else {
        return STAGING_FAILED;
    };
    let staging = staging_buffer.as_raw();
    let api = unsafe { vtable::<ID3D11DeviceContext>(&context) };
    unsafe { (api.CopyResource)(context, staging, buffer) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if unsafe { (api.Map)(context, staging, 0, D3D11_MAP_READ, 0, &mut mapped) }.is_err() {
        return MAP_FAILED;
    }
    let result = if mapped.pData.is_null() {
        MAP_FAILED
    } else {
        unsafe {
            ptr::copy_nonoverlapping(
                mapped.pData.cast::<u8>(),
                destination.cast::<u8>(),
                bytes as usize,
            )
        };
        OK
    };
    unsafe { (api.Unmap)(context, staging, 0) };
    result
}
