// SPDX-License-Identifier: MIT
//! Window, WARP device and check helpers the Windows fixture crates share.
#![cfg(windows)]

use std::ffi::{CStr, OsStr};
use windows::Win32::Foundation::{
    E_POINTER, FreeLibrary, HINSTANCE, HMODULE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, D3D11CreateDevice, D3D11CreateDeviceAndSwapChain,
    ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, D3D12GetDebugInterface, ID3D12Debug, ID3D12Device,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_SWAP_CHAIN_DESC, IDXGIAdapter, IDXGIFactory4, IDXGISwapChain,
};
use windows::Win32::System::LibraryLoader::{
    GetModuleHandleW, GetProcAddress, LOAD_LIBRARY_FLAGS, LoadLibraryExW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, UnregisterClassW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
};
use windows::core::{HSTRING, PCSTR, PCWSTR, Result};

/// Ends the fixture process at the first failed condition, naming the file and line.
#[macro_export]
macro_rules! check {
    ($condition:expr) => {
        if !$condition {
            eprintln!(
                "check failed at {}:{}: {}",
                file!(),
                line!(),
                stringify!($condition)
            );
            std::process::exit(1);
        }
    };
}

/// A D3D11 device and its immediate context on `driver`. Fixtures that prefer hardware call this
/// with the hardware driver first and [`warp_d3d11`] second.
pub fn d3d11_device(
    driver: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let (mut device, mut context) = (None, None);
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            flags,
            levels,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    device.zip(context).ok_or_else(|| E_POINTER.into())
}

pub fn warp_d3d11(
    flags: D3D11_CREATE_DEVICE_FLAG,
    levels: Option<&[D3D_FEATURE_LEVEL]>,
) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    d3d11_device(D3D_DRIVER_TYPE_WARP, flags, levels)
}

/// A D3D11 device, its context and a swap chain for `desc.OutputWindow` on `driver`.
pub fn d3d11_swapchain(
    driver: D3D_DRIVER_TYPE,
    flags: D3D11_CREATE_DEVICE_FLAG,
    desc: &DXGI_SWAP_CHAIN_DESC,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain)> {
    let (mut chain, mut device, mut context) = (None, None, None);
    unsafe {
        D3D11CreateDeviceAndSwapChain(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(desc),
            Some(&mut chain),
            Some(&mut device),
            None,
            Some(&mut context),
        )
    }?;
    match (device, context, chain) {
        (Some(device), Some(context), Some(chain)) => Ok((device, context, chain)),
        _ => Err(E_POINTER.into()),
    }
}

pub fn warp_swapchain(
    flags: D3D11_CREATE_DEVICE_FLAG,
    desc: &DXGI_SWAP_CHAIN_DESC,
) -> Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain)> {
    d3d11_swapchain(D3D_DRIVER_TYPE_WARP, flags, desc)
}

/// Turns the D3D12 debug layer on; false when it is not installed.
pub fn enable_d3d12_debug_layer() -> bool {
    let mut layer: Option<ID3D12Debug> = None;
    if unsafe { D3D12GetDebugInterface(&mut layer) }.is_err() {
        return false;
    }
    layer.is_some_and(|layer| {
        unsafe { layer.EnableDebugLayer() };
        true
    })
}

/// A D3D12 device on the WARP adapter, with its factory.
pub fn warp_d3d12() -> Result<(IDXGIFactory4, ID3D12Device)> {
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }?;
    let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }?;
    let mut device = None;
    unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }?;
    let device = device.ok_or_else(|| windows::core::Error::from(E_POINTER))?;
    Ok((factory, device))
}

/// The windows crate wraps DefWindowProcW in a Rust function; a window class needs the system ABI.
pub unsafe extern "system" fn default_window_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// A window of its own class, destroyed and unregistered on drop.
pub struct Window {
    pub handle: HWND,
    class: PCWSTR,
    instance: HINSTANCE,
}

impl Window {
    pub fn new(
        class: PCWSTR,
        title: PCWSTR,
        style: WINDOW_STYLE,
        width: i32,
        height: i32,
    ) -> Result<Self> {
        let instance = HINSTANCE(unsafe { GetModuleHandleW(PCWSTR::null())? }.0);
        let description = WNDCLASSW {
            lpfnWndProc: Some(default_window_procedure),
            hInstance: instance,
            lpszClassName: class,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&description) } == 0 {
            return Err(windows::core::Error::from_thread());
        }
        let handle = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                title,
                style,
                0,
                0,
                width,
                height,
                None,
                None,
                Some(instance),
                None,
            )
        };
        match handle {
            Ok(handle) => Ok(Self {
                handle,
                class,
                instance,
            }),
            Err(error) => {
                let _ = unsafe { UnregisterClassW(class, Some(instance)) };
                Err(error)
            }
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.handle);
            let _ = UnregisterClassW(self.class, Some(self.instance));
        }
    }
}

/// A DLL loaded for the life of the value, for fixtures that call the exports of a shipped binary.
pub struct Library(HMODULE);

impl Library {
    /// Loads `path`; `LOAD_LIBRARY_FLAGS(0)` is the default search order.
    pub fn open(path: &OsStr, flags: LOAD_LIBRARY_FLAGS) -> Result<Self> {
        Ok(Self(unsafe {
            LoadLibraryExW(&HSTRING::from(path), None, flags)
        }?))
    }

    pub fn symbol(&self, name: &CStr) -> Option<unsafe extern "system" fn() -> isize> {
        unsafe { GetProcAddress(self.0, PCSTR(name.as_ptr().cast())) }
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(self.0) };
    }
}
