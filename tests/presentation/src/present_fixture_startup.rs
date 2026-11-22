// SPDX-License-Identifier: MIT
//! Shared startup helpers for the two linked presentation executables.
use crate::fixture_device::{
    rsf_present_fixture_adapter, rsf_present_fixture_device11, rsf_present_fixture_device12,
};
use std::ffi::{CStr, c_char, c_void};
use std::ptr::null_mut;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext};
use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC, IDXGIAdapter1, IDXGIFactory, IDXGISwapChain,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{IUnknown, Interface, PCWSTR};

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

pub(super) fn adapter<T: Interface>(
    vendor: u32,
    factory4: bool,
) -> Result<(T, IDXGIAdapter1), i32> {
    let (mut factory, mut adapter) = (null_mut(), null_mut());
    let status = unsafe {
        rsf_present_fixture_adapter(vendor, u32::from(factory4), &mut factory, &mut adapter)
    };
    if status != 0 {
        return Err(status);
    }
    Ok(unsafe { (T::from_raw(factory), IDXGIAdapter1::from_raw(adapter)) })
}

pub(super) fn device11(
    adapter: &IDXGIAdapter1,
) -> Result<(ID3D11Device, ID3D11DeviceContext), i32> {
    let (mut device, mut context) = (null_mut(), null_mut());
    if unsafe { rsf_present_fixture_device11(adapter.as_raw(), &mut device, &mut context) } != 0 {
        return Err(1);
    }
    Ok(unsafe {
        (
            ID3D11Device::from_raw(device),
            ID3D11DeviceContext::from_raw(context),
        )
    })
}

pub(super) fn device12(adapter: &IDXGIAdapter1) -> Result<(ID3D12Device, ID3D12CommandQueue), i32> {
    let (mut device, mut queue) = (null_mut(), null_mut());
    if unsafe { rsf_present_fixture_device12(adapter.as_raw(), &mut device, &mut queue) } != 0 {
        return Err(1);
    }
    Ok(unsafe {
        (
            ID3D12Device::from_raw(device),
            ID3D12CommandQueue::from_raw(queue),
        )
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

pub(super) fn register_window(class: PCWSTR) -> Result<HINSTANCE, i32> {
    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|_| 1)?;
    let instance = HINSTANCE(module.0);
    let wc = WNDCLASSW {
        hInstance: instance,
        lpfnWndProc: Some(default_window_procedure),
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err(1);
    }
    Ok(instance)
}

pub(super) fn window(class: PCWSTR, title: PCWSTR, instance: HINSTANCE) -> Result<HWND, i32> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class,
            title,
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            1280,
            720,
            None,
            None,
            Some(instance),
            None,
        )
    }
    .map_err(|_| 1)
}

pub(super) fn show(window: HWND, foreground: bool) {
    let _ = unsafe { ShowWindow(window, SW_SHOW) };
    if foreground {
        let _ = unsafe { SetForegroundWindow(window) };
    }
}

// The windows crate wraps DefWindowProcW in a Rust function; the class needs the system ABI.
unsafe extern "system" fn default_window_procedure(
    window: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}
