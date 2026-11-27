// SPDX-License-Identifier: MIT
//! Shared startup helpers for the two linked presentation executables.
use rsf_test_support::d3d11_device_on;
use rsf_test_support::d3d12::{device_and_queue, validation_errors};
use rsf_test_support::hardware_adapter;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_SWAP_CHAIN_DESC, IDXGIAdapter1, IDXGIFactory, IDXGIFactory1,
    IDXGISwapChain,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{IUnknown, Interface};

pub(super) type Raw = *mut c_void;

unsafe extern "C" {
    fn strtoul(text: *const c_char, end: *mut *mut c_char, base: i32) -> u32;
    fn getenv(name: *const c_char) -> *mut c_char;
}

pub(super) unsafe fn args<'a>(argc: i32, argv: *const *const c_char) -> &'a [*const c_char] {
    unsafe { std::slice::from_raw_parts(argv, argc as usize) }
}

pub(super) unsafe fn number(text: *const c_char) -> u32 {
    unsafe { strtoul(text, null_mut(), 0) }
}

pub(super) fn env_present(name: &CStr) -> bool {
    !unsafe { getenv(name.as_ptr()) }.is_null()
}

/// The factory `T` and the hardware adapter of `vendor`. A missing adapter is the skip status 77.
pub(super) fn adapter<T: Interface>(vendor: u32) -> Result<(T, IDXGIAdapter1), i32> {
    let factory = unsafe { CreateDXGIFactory1::<T>() }.map_err(|_| 1)?;
    let found = hardware_adapter(&factory.cast::<IDXGIFactory1>().map_err(|_| 1)?, vendor)
        .map_err(|_| 1)?;
    Ok((factory, found.ok_or(77)?))
}

pub(super) fn device11(
    adapter: &IDXGIAdapter1,
) -> Result<(ID3D11Device, ID3D11DeviceContext), i32> {
    d3d11_device_on(
        Some(&**adapter),
        D3D_DRIVER_TYPE_UNKNOWN,
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
    )
    .map_err(|_| 1)
}

pub(super) fn device12(adapter: &IDXGIAdapter1) -> Result<(ID3D12Device, ID3D12CommandQueue), i32> {
    device_and_queue(adapter).map_err(|_| 1)
}

/// Prints the D3D12 validation errors of `device`. True when there are none; a device without an
/// info queue is not clean.
pub(super) fn validation_clean(device: &ID3D12Device) -> bool {
    validation_errors(device).is_some_and(|errors| {
        errors
            .iter()
            .for_each(|description| println!("D3D12 validation: {description}"));
        errors.is_empty()
    })
}

pub(super) fn create_chain(
    factory: &IDXGIFactory,
    producer: Raw,
    description: &DXGI_SWAP_CHAIN_DESC,
) -> Result<IDXGISwapChain, i32> {
    // The factory receives the supplied device or queue at its original interface address.
    let device = unsafe { IUnknown::from_raw_borrowed(&producer) }.ok_or(1)?;
    let mut chain = None;
    unsafe { factory.CreateSwapChain(device, description, &mut chain) }
        .ok()
        .map_err(|_| 1)?;
    chain.ok_or(1)
}

pub(super) fn show(window: HWND, foreground: bool) {
    let _ = unsafe { ShowWindow(window, SW_SHOW) };
    if foreground {
        let _ = unsafe { SetForegroundWindow(window) };
    }
}
