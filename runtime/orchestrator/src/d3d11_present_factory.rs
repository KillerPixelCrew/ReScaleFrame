// SPDX-License-Identifier: MIT
//! The native detour retains its trampoline, while Rust owns descriptor
//! translation and the decision to intercept a factory call.
use super::{
    DXGI_ERROR_UNSUPPORTED, E_FAIL, E_INVALIDARG, Platform, S_OK, failed, log,
    rsf_core_d3d11p_capture, rsf_core_d3d11p_captured, rsf_core_d3d11p_create,
};
use crate::com_ref::ComRef;
use crate::d3d11_present_abi::{Raw, rsf_d3d11p_plain_plan, rsf_d3d11p_request};
use std::ptr::{self, null_mut};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED, DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FULLSCREEN_DESC, DXGI_SWAP_EFFECT,
    DXGI_USAGE, IDXGIFactory, IDXGIFactory2, IDXGISwapChain, IDXGISwapChain1, IDXGISwapChain4,
};
use windows::core::{BOOL, Interface};

const _: () = {
    assert!(size_of::<DXGI_SWAP_CHAIN_DESC>() == 72);
    assert!(size_of::<DXGI_SWAP_CHAIN_DESC1>() == 48);
    assert!(size_of::<DXGI_SWAP_CHAIN_FULLSCREEN_DESC>() == 20);
};

fn translate(
    hwnd: HWND,
    desc: &DXGI_SWAP_CHAIN_DESC1,
    fullscreen: Option<&DXGI_SWAP_CHAIN_FULLSCREEN_DESC>,
) -> DXGI_SWAP_CHAIN_DESC {
    DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: desc.Width,
            Height: desc.Height,
            RefreshRate: fullscreen.map_or_else(Default::default, |mode| mode.RefreshRate),
            Format: desc.Format,
            ScanlineOrdering: fullscreen.map_or(DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, |mode| {
                mode.ScanlineOrdering
            }),
            Scaling: fullscreen.map_or(DXGI_MODE_SCALING_UNSPECIFIED, |mode| mode.Scaling),
        },
        SampleDesc: desc.SampleDesc,
        BufferUsage: desc.BufferUsage,
        BufferCount: desc.BufferCount,
        OutputWindow: hwnd,
        Windowed: fullscreen.map_or(BOOL(1), |mode| mode.Windowed),
        SwapEffect: desc.SwapEffect,
        Flags: desc.Flags,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_create(
    platform: *const Platform,
    factory: Raw,
    device: Raw,
    desc: &DXGI_SWAP_CHAIN_DESC,
    out: *mut Raw,
) -> i32 {
    let mode = &desc.BufferDesc;
    let request = rsf_d3d11p_request {
        window: desc.OutputWindow.0,
        width: mode.Width,
        height: mode.Height,
        format: mode.Format.0 as u32,
        sample_count: desc.SampleDesc.Count,
        buffer_count: desc.BufferCount,
        windowed: u32::from(desc.Windowed.0 != 0),
        flags: desc.Flags,
        swap_effect: desc.SwapEffect.0 as u32,
        refresh_numerator: mode.RefreshRate.Numerator,
        refresh_denominator: mode.RefreshRate.Denominator,
    };
    unsafe {
        rsf_core_d3d11p_create(
            platform,
            factory,
            device,
            &request,
            ptr::from_ref(desc).cast(),
            out,
        )
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_intercept(
    platform: *const Platform,
    factory: Raw,
    device: Raw,
    desc: *const DXGI_SWAP_CHAIN_DESC,
    out: *mut Raw,
) -> bool {
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return false;
    };
    if rsf_core_d3d11p_capture(desc.OutputWindow.0) == 0
        || failed(unsafe { rsf_d3d11p_factory_create(platform, factory, device, desc, out) })
    {
        return false;
    }
    unsafe { rsf_core_d3d11p_captured(platform, out.read()) };
    true
}

unsafe fn create_hwnd(
    platform: *const Platform,
    factory: Raw,
    device: Raw,
    desc: &DXGI_SWAP_CHAIN_DESC,
    out: *mut Raw,
    capture: bool,
) -> Result<i32, i32> {
    let mut bridge = null_mut();
    let status = unsafe { rsf_d3d11p_factory_create(platform, factory, device, desc, &mut bridge) };
    if failed(status) {
        return Err(status);
    }
    let bridge = unsafe { IDXGISwapChain::from_raw(bridge) };
    // Retain raw HRESULT semantics for a null output and transfer one reference.
    let result = unsafe { bridge.query(&IDXGISwapChain1::IID, out) }.0;
    if capture && !failed(result) {
        unsafe { rsf_core_d3d11p_captured(platform, bridge.as_raw()) };
    }
    Ok(result)
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_d3d11p_factory_intercept_hwnd(
    platform: *const Platform,
    factory: Raw,
    device: Raw,
    hwnd: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    fullscreen: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    output: Raw,
    out: *mut Raw,
    result: *mut i32,
) -> bool {
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return false;
    };
    if !output.is_null() || desc.Stereo.0 != 0 || rsf_core_d3d11p_capture(hwnd.0) == 0 {
        return false;
    }
    let translated = translate(hwnd, desc, unsafe { fullscreen.as_ref() });
    match unsafe { create_hwnd(platform, factory, device, &translated, out, true) } {
        Ok(status) => {
            unsafe { result.write(status) };
            true
        }
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_create_hwnd(
    platform: *const Platform,
    factory: Raw,
    device: Raw,
    hwnd: HWND,
    desc: *const DXGI_SWAP_CHAIN_DESC1,
    out: *mut Raw,
) -> i32 {
    if factory.is_null() || device.is_null() || desc.is_null() || out.is_null() {
        return E_INVALIDARG;
    }
    unsafe { out.write(null_mut()) };
    let desc = unsafe { &*desc };
    if desc.Stereo.0 != 0 {
        return DXGI_ERROR_UNSUPPORTED;
    }
    let translated = translate(hwnd, desc, None);
    unsafe { create_hwnd(platform, factory, device, &translated, out, false) }
        .unwrap_or_else(|error| error)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_hook_targets(
    factory: Raw,
    create: *mut Raw,
    create_hwnd: *mut Raw,
) {
    let factory2 = unsafe { IDXGIFactory2::from_raw_borrowed(&factory) }.unwrap();
    let factory1 = unsafe { IDXGIFactory::from_raw_borrowed(&factory) }.unwrap();
    unsafe {
        create.write(factory1.vtable().CreateSwapChain as Raw);
        create_hwnd.write(factory2.vtable().CreateSwapChainForHwnd as Raw);
    }
}

type GuardedCreate = unsafe extern "C" fn(Raw, Raw, *mut DXGI_SWAP_CHAIN_DESC, *mut Raw) -> i32;

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_d3d11p_factory_create_plain(
    facade: Raw,
    factory: Raw,
    description: &DXGI_SWAP_CHAIN_DESC,
    plan: &rsf_d3d11p_plain_plan,
    native12: u32,
    original: Option<GuardedCreate>,
    out: *mut Raw,
    queue: *mut Raw,
) -> i32 {
    let mut desc = *description;
    desc.SwapEffect = DXGI_SWAP_EFFECT(plan.swap_effect as i32);
    desc.Flags = plan.flags;
    if plan.buffer_count != 0 {
        desc.BufferCount = plan.buffer_count;
    }
    if plan.buffer_usage != 0 {
        desc.BufferUsage = DXGI_USAGE(plan.buffer_usage);
    }
    let mut candidate = null_mut();
    let result = match original {
        Some(create) => unsafe { create(factory, plan.queue, &mut desc, &mut candidate) },
        None => {
            let bound = unsafe { IDXGIFactory::from_raw_borrowed(&factory) }.unwrap();
            unsafe { (bound.vtable().CreateSwapChain)(factory, plan.queue, &desc, &mut candidate) }
                .0
        }
    };
    let chain = unsafe { ComRef::from_owned(candidate) };
    if failed(result) {
        let mode = &desc.BufferDesc;
        log(&format!(
            "Physical chain creation refused HRESULT=0x{:08x} facade={:p} factory={:p} queue={:p} window={:p} size={}x{} format={} buffers={} effect={} flags=0x{:x} native12={} usage=0x{:x} sample={}/{} refresh={}/{} scanline={} scaling={} windowed={}",
            result as u32,
            facade,
            factory,
            plan.queue,
            desc.OutputWindow.0,
            mode.Width,
            mode.Height,
            mode.Format.0 as u32,
            desc.BufferCount,
            desc.SwapEffect.0 as u32,
            desc.Flags,
            native12,
            desc.BufferUsage.0,
            desc.SampleDesc.Count,
            desc.SampleDesc.Quality,
            mode.RefreshRate.Numerator,
            mode.RefreshRate.Denominator,
            mode.ScanlineOrdering.0 as u32,
            mode.Scaling.0 as u32,
            u32::from(desc.Windowed.0 != 0)
        ));
        return E_FAIL;
    }
    let Some(chain) = chain else {
        return E_FAIL;
    };
    let bound = unsafe { IDXGISwapChain::from_raw_borrowed(&candidate) }.unwrap();
    let mut physical = null_mut();
    let queried = unsafe { bound.query(&IDXGISwapChain4::IID, &mut physical) };
    let replacement = unsafe { ComRef::from_owned(physical) };
    // Keep the creation reference live through QI, even on provider refusal.
    drop(chain);
    let Some(replacement) = replacement else {
        return E_FAIL;
    };
    if !queried.is_ok() {
        return E_FAIL;
    }
    unsafe {
        out.write(replacement.into_raw());
        queue.write(plan.queue);
    }
    S_OK
}
