// SPDX-License-Identifier: MIT
//! Native COM discovery for the shared D3D presentation platform.
use super::{E_FAIL, E_INVALIDARG, S_OK};
use crate::d3d11_present_abi::Raw;
use std::ptr;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_DEVICE_CONTEXT_IMMEDIATE, ID3D11Device, ID3D11DeviceContext, ID3D11DeviceContext4,
};
use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12CommandQueue, ID3D12Device};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_FEATURE_PRESENT_ALLOW_TEARING,
    DXGI_SWAP_CHAIN_DESC, IDXGIAdapter1, IDXGIDevice, IDXGIFactory, IDXGIFactory4, IDXGIFactory5,
    IDXGISwapChain,
};
use windows::core::{BOOL, IUnknown, Interface, Result};

const NO_INTERFACE: i32 = windows::Win32::Foundation::E_NOINTERFACE.0;

unsafe fn borrow<T: Interface>(raw: &Raw) -> &T {
    unsafe { T::from_raw_borrowed(raw) }.unwrap()
}

fn same_identity(left: &impl Interface, right: &impl Interface) -> bool {
    let Ok(left) = left.cast::<IUnknown>() else {
        return false;
    };
    right
        .cast::<IUnknown>()
        .is_ok_and(|right| left.as_raw() == right.as_raw())
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_queue_device(queue: Raw, out: *mut Raw) -> i32 {
    let mut device: Option<ID3D12Device> = None;
    let queue = unsafe { borrow::<ID3D12CommandQueue>(&queue) };
    if unsafe { queue.GetDevice(&mut device) }.is_err() {
        return NO_INTERFACE;
    }
    let Some(device) = device else {
        return NO_INTERFACE;
    };
    unsafe { out.write(device.into_raw()) };
    S_OK
}

unsafe fn adapter_at_device(device: &ID3D12Device) -> Result<IDXGIAdapter1> {
    let factory = unsafe { CreateDXGIFactory1::<IDXGIFactory4>() }?;
    unsafe { factory.EnumAdapterByLuid(device.GetAdapterLuid()) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_native_adapter(
    device: Raw,
    out: *mut Raw,
) -> i32 {
    let Ok(adapter) = (unsafe { adapter_at_device(borrow::<ID3D12Device>(&device)) }) else {
        return E_FAIL;
    };
    unsafe { out.write(adapter.into_raw()) };
    S_OK
}

#[repr(C)]
#[derive(Default)]
pub(super) struct AdapterInfo {
    pub(super) vendor: u32,
    pub(super) device: u32,
    pub(super) software: u32,
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_read_adapter(
    source: Raw,
    native_device: Raw,
    out: *mut AdapterInfo,
) -> i32 {
    let adapter = unsafe {
        if native_device.is_null() {
            borrow::<IUnknown>(&source)
                .cast::<IDXGIDevice>()
                .and_then(|dxgi| dxgi.GetAdapter())
                .and_then(|adapter| adapter.cast::<IDXGIAdapter1>())
        } else {
            adapter_at_device(borrow::<ID3D12Device>(&native_device))
        }
    };
    let Ok(desc) = adapter.and_then(|adapter| unsafe { adapter.GetDesc1() }) else {
        return E_FAIL;
    };
    unsafe {
        out.write(AdapterInfo {
            vendor: desc.VendorId,
            device: desc.DeviceId,
            software: u32::from(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0),
        });
    }
    S_OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_immediate(
    source: Raw,
    out_device: *mut Raw,
    out_context: *mut Raw,
    out_adapter: *mut Raw,
) -> i32 {
    let Ok(device) = (unsafe { borrow::<IUnknown>(&source) }).cast::<ID3D11Device>() else {
        return NO_INTERFACE;
    };
    // The capsule owns partial initialization until its existing cleanup runs.
    let raw_device = device.into_raw();
    unsafe { out_device.write(raw_device) };
    let device = unsafe { borrow::<ID3D11Device>(&raw_device) };
    let context = unsafe { device.GetImmediateContext() }
        .and_then(|immediate| immediate.cast::<ID3D11DeviceContext4>());
    let Ok(context) = context else {
        return NO_INTERFACE;
    };
    unsafe { out_context.write(context.into_raw()) };
    let Ok(adapter) = device
        .cast::<IDXGIDevice>()
        .and_then(|dxgi| unsafe { dxgi.GetAdapter() })
    else {
        return NO_INTERFACE;
    };
    unsafe { out_adapter.write(adapter.into_raw()) };
    S_OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_create12(adapter: Raw, out: *mut Raw) -> i32 {
    let adapter = unsafe { IUnknown::from_raw_borrowed(&adapter) };
    let mut device: Option<ID3D12Device> = None;
    if unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.is_err() {
        return super::DXGI_ERROR_UNSUPPORTED;
    }
    let Some(device) = device else {
        return super::DXGI_ERROR_UNSUPPORTED;
    };
    unsafe { out.write(device.into_raw()) };
    S_OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_tearing(factory: Raw) -> u32 {
    let Ok(facility) = (unsafe { borrow::<IDXGIFactory>(&factory) }).cast::<IDXGIFactory5>() else {
        return 0;
    };
    let mut allow = BOOL(0);
    let result = unsafe {
        facility.CheckFeatureSupport(
            DXGI_FEATURE_PRESENT_ALLOW_TEARING,
            ptr::from_mut(&mut allow).cast(),
            size_of::<BOOL>() as u32,
        )
    };
    u32::from(result.is_ok() && allow.0 != 0)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_device_verify_outer(
    inner: Raw,
    window: Raw,
    device: Raw,
    context: Raw,
    outer: Raw,
) -> i32 {
    // The scoped wrapper can reenter COM. Only compare canonical identities of
    // the caller's live inputs and keep each QI's reference through the check.
    let (Some(inner), Some(outer), Some(device), Some(context)) = (unsafe {
        (
            IDXGISwapChain::from_raw_borrowed(&inner),
            IDXGISwapChain::from_raw_borrowed(&outer),
            IUnknown::from_raw_borrowed(&device),
            ID3D11DeviceContext::from_raw_borrowed(&context),
        )
    }) else {
        return E_INVALIDARG;
    };
    if !same_identity(outer, inner)
        || !unsafe { outer.GetDesc() }.is_ok_and(|desc| desc.OutputWindow.0 == window)
    {
        return E_INVALIDARG;
    }
    let Ok(captured_device) = (unsafe { inner.GetDevice::<ID3D11Device>() }) else {
        return E_INVALIDARG;
    };
    let Ok(outer_device) = (unsafe { outer.GetDevice::<ID3D11Device>() }) else {
        return E_INVALIDARG;
    };
    if unsafe { context.GetType() } != D3D11_DEVICE_CONTEXT_IMMEDIATE {
        return E_INVALIDARG;
    }
    let Ok(context_device) = (unsafe { context.GetDevice() }) else {
        return E_INVALIDARG;
    };
    if same_identity(&captured_device, device)
        && same_identity(&captured_device, &outer_device)
        && same_identity(&captured_device, &context_device)
    {
        S_OK
    } else {
        E_INVALIDARG
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_d3d11p_device_native_chain(
    device: Raw,
    chain: Raw,
    desc: *mut DXGI_SWAP_CHAIN_DESC,
    factory: *mut Raw,
) -> i32 {
    let chain = unsafe { borrow::<IDXGISwapChain>(&chain) };
    let Ok(description) = (unsafe { chain.GetDesc() }) else {
        return E_INVALIDARG;
    };
    let Ok(owner) = (unsafe { chain.GetDevice::<ID3D11Device>() }) else {
        return E_INVALIDARG;
    };
    if !same_identity(unsafe { borrow::<ID3D11Device>(&device) }, &owner) {
        return E_INVALIDARG;
    }
    let Ok(parent) = (unsafe { chain.GetParent::<IDXGIFactory>() }) else {
        return E_INVALIDARG;
    };
    unsafe {
        desc.write(description);
        factory.write(parent.into_raw());
    }
    S_OK
}
