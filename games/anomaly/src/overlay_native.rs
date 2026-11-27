// SPDX-License-Identifier: MIT
//! Windows SDK calls used by the Anomaly overlay's D3D11 and DirectInput boundaries.
// The overlay calls the chain and capture leaves directly; its unit tests swap in fakes.
#![cfg_attr(test, allow(dead_code))]
use crate::lease::Lease;
use crate::overlay::ChainInfo;
use crate::sys::{
    rsf_input_dump_named_texture_fn, rsf_named_frame_dump, rsf_named_frame_dump_report,
};
use std::ffi::c_void;
use std::mem::transmute;
use std::ptr::{null, null_mut};
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Texture2D};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGISwapChain,
};
use windows::core::Interface;
use windows_sys::Win32::Foundation::{HMODULE, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::core::{GUID, HRESULT};

pub(super) type Object = *mut c_void;
type Symbol = unsafe extern "system" fn() -> isize;
const SLOT_CREATE_DEVICE: usize = 3;

pub(super) unsafe fn slot(object: Object, index: usize) -> usize {
    let table = unsafe { *object.cast::<*const usize>() };
    unsafe { *table.add(index) }
}

pub(super) use crate::com::release;

pub(super) type CreateDevice =
    unsafe extern "system" fn(Object, *const GUID, *mut Object, Object) -> HRESULT;

pub(super) unsafe fn create_device(input: Object, guid: *const GUID) -> Object {
    let create: CreateDevice = unsafe { transmute(slot(input, SLOT_CREATE_DEVICE)) };
    let mut device = null_mut();
    if unsafe { create(input, guid, &mut device, null_mut()) } < 0 {
        return null_mut();
    }
    device
}

pub(super) unsafe fn create_input(address: Symbol, iid: *const GUID, version: u32) -> Object {
    type Create =
        unsafe extern "system" fn(HMODULE, u32, *const GUID, *mut Object, Object) -> HRESULT;
    let create: Create = unsafe { transmute(address) };
    let mut input = null_mut();
    if unsafe {
        create(
            GetModuleHandleW(null()),
            version,
            iid,
            &mut input,
            null_mut(),
        )
    } < 0
    {
        return null_mut();
    }
    input
}

/// A WARP swap chain on `window` from `D3D11CreateDeviceAndSwapChain` at `address`, for its vtables. The device and
/// context references are released here; the chain holds its own.
pub(super) unsafe fn create_warp_chain(
    address: Symbol,
    window: HWND,
    extent: u32,
) -> Option<IDXGISwapChain> {
    type Create = unsafe extern "system" fn(
        Object,
        u32,
        HMODULE,
        u32,
        *const u32,
        u32,
        u32,
        *const DXGI_SWAP_CHAIN_DESC,
        *mut Object,
        *mut Object,
        *mut u32,
        *mut Object,
    ) -> HRESULT;
    const WARP: u32 = 5;
    const SDK_VERSION: u32 = 7;
    let create: Create = unsafe { transmute(address) };
    let description = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: extent,
            Height: extent,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: windows::Win32::Foundation::HWND(window),
        Windowed: true.into(),
        ..Default::default()
    };
    let (mut chain, mut device, mut context) = (null_mut(), null_mut(), null_mut());
    let result = unsafe {
        create(
            null_mut(),
            WARP,
            null_mut(),
            0,
            null(),
            0,
            SDK_VERSION,
            &description,
            &mut chain,
            &mut device,
            null_mut(),
            &mut context,
        )
    };
    let _device = Lease::new(device, release);
    let _context = Lease::new(context, release);
    // Every returned reference is owned, also on failure.
    let chain = (!chain.is_null()).then(|| unsafe { IDXGISwapChain::from_raw(chain) });
    chain.filter(|_| result >= 0)
}

/// The engine chain's window, extent and device identity (compared only; its reference is released here).
pub(super) unsafe fn describe_chain(chain: Object) -> Option<ChainInfo> {
    let chain = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }?;
    let desc = unsafe { chain.GetDesc() }.ok()?;
    let device = unsafe { chain.GetDevice::<ID3D11Device>() }.ok();
    Some(ChainInfo {
        window: desc.OutputWindow.0,
        device: device
            .as_ref()
            .map_or(null(), |device| device.as_raw().cast_const()),
        width: desc.BufferDesc.Width,
        height: desc.BufferDesc.Height,
    })
}

pub(super) unsafe fn capture_backbuffer(
    chain: Object,
    dump: rsf_input_dump_named_texture_fn,
    request: *const rsf_named_frame_dump,
    report: *mut rsf_named_frame_dump_report,
) -> i32 {
    let Some(dump) = dump else { return -1 };
    let Some(chain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }) else {
        return -1;
    };
    let Ok(device) = (unsafe { chain.GetDevice::<ID3D11Device>() }) else {
        return -1;
    };
    let Ok(context) = (unsafe { device.GetImmediateContext() }) else {
        return -1;
    };
    match unsafe { chain.GetBuffer::<ID3D11Texture2D>(0) } {
        Ok(buffer) => unsafe { dump(context.as_raw(), buffer.as_raw(), request, report) },
        Err(_) => -1,
    }
}
