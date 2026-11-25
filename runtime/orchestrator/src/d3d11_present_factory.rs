// SPDX-License-Identifier: MIT
//! Descriptor translation, interception and cold installation for DXGI factories.
use super::device::rsf_d3d11p_device_native_chain;
use super::{
    DXGI_ERROR_UNSUPPORTED, E_FAIL, E_INVALIDARG, E_UNEXPECTED, S_OK, failed, log,
    rsf_core_d3d11p_active_bridge, rsf_core_d3d11p_capture, rsf_core_d3d11p_captured,
    rsf_core_d3d11p_create, rsf_core_d3d11p_prepare, rsf_core_d3d11p_set_scoped,
    rsf_d3d11_present_is_owner, settings,
};
use crate::com_ref;
use crate::d3d11_present_abi::{Raw, rsf_d3d11p_plain_plan, rsf_d3d11p_request};
use crate::sys::rsf_d3d11_present_setup;
use rsf_minhook_sys::{
    MH_CreateHook, MH_DisableHook, MH_ERROR_ALREADY_INITIALIZED, MH_ERROR_NOT_CREATED,
    MH_ERROR_NOT_INITIALIZED, MH_EnableHook, MH_Initialize, MH_OK, MH_RemoveHook,
};
use std::ffi::c_void;
use std::ptr::{self, null_mut};
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
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

unsafe fn rsf_d3d11p_factory_create(
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
    unsafe { rsf_core_d3d11p_create(factory, device, &request, ptr::from_ref(desc).cast(), out) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_intercept(
    factory: Raw,
    device: Raw,
    desc: *const DXGI_SWAP_CHAIN_DESC,
    out: *mut Raw,
) -> bool {
    let Some(desc) = (unsafe { desc.as_ref() }) else {
        return false;
    };
    if rsf_core_d3d11p_capture(desc.OutputWindow.0) == 0
        || failed(unsafe { rsf_d3d11p_factory_create(factory, device, desc, out) })
    {
        return false;
    }
    unsafe { rsf_core_d3d11p_captured(out.read()) };
    true
}

unsafe fn create_hwnd(
    factory: Raw,
    device: Raw,
    desc: &DXGI_SWAP_CHAIN_DESC,
    out: *mut Raw,
    capture: bool,
) -> Result<i32, i32> {
    let mut bridge = null_mut();
    let status = unsafe { rsf_d3d11p_factory_create(factory, device, desc, &mut bridge) };
    if failed(status) {
        return Err(status);
    }
    let bridge = unsafe { IDXGISwapChain::from_raw(bridge) };
    // Retain raw HRESULT semantics for a null output and transfer one reference.
    let result = unsafe { bridge.query(&IDXGISwapChain1::IID, out) }.0;
    if capture && !failed(result) {
        unsafe { rsf_core_d3d11p_captured(bridge.as_raw()) };
    }
    Ok(result)
}

#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_d3d11p_factory_intercept_hwnd(
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
    match unsafe { create_hwnd(factory, device, &translated, out, true) } {
        Ok(status) => {
            unsafe { result.write(status) };
            true
        }
        Err(_) => false,
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_create_hwnd(
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
    unsafe { create_hwnd(factory, device, &translated, out, false) }.unwrap_or_else(|error| error)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11p_factory_wrap_native(
    device: Raw,
    native: Raw,
    out: *mut Raw,
) -> i32 {
    if out.is_null() {
        return E_INVALIDARG;
    }
    unsafe { out.write(null_mut()) };
    if device.is_null()
        || native.is_null()
        || settings().accept_window.is_none()
        || rsf_d3d11p_factory_hooks_block_explicit() != 0
        || !rsf_core_d3d11p_active_bridge().is_null()
    {
        return E_INVALIDARG;
    }
    let mut desc = DXGI_SWAP_CHAIN_DESC::default();
    let mut factory = null_mut();
    if failed(unsafe { rsf_d3d11p_device_native_chain(device, native, &mut desc, &mut factory) }) {
        return E_INVALIDARG;
    }
    let factory = unsafe { IDXGIFactory::from_raw(factory) };
    let mut facade = null_mut();
    let result = unsafe { rsf_d3d11p_factory_create(factory.as_raw(), device, &desc, &mut facade) };
    let owned = unsafe { com_ref::owned(facade) };
    if failed(result) {
        return result;
    }
    let Some(owned) = owned else {
        return E_UNEXPECTED;
    };
    if unsafe { rsf_d3d11_present_is_owner(owned.as_raw()) } == 0 {
        return E_UNEXPECTED;
    }
    unsafe { out.write(owned.into_raw()) };
    S_OK
}

// The SEH-guarded factory bootstrap and the two detours, in d3d11_present_bridge.c.
unsafe extern "C" {
    fn rsf_d3d11p_bootstrap_factory() -> Raw;
    fn rsf_d3d11p_create_hook();
    fn rsf_d3d11p_hwnd_hook();
}

const IDLE: u8 = 0;
const INSTALLING: u8 = 1;
const READY: u8 = 2;
const BLOCKED: u8 = 3;
static HOOK_STATE: AtomicU8 = AtomicU8::new(IDLE);
static LEGACY_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static HWND_ORIGINAL: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_d3d11p_factory_hooks_ready() -> u32 {
    u32::from(HOOK_STATE.load(Ordering::Acquire) == READY)
}

pub(super) extern "C" fn rsf_d3d11p_factory_hooks_block_explicit() -> u32 {
    u32::from(HOOK_STATE.load(Ordering::Acquire) != IDLE)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11p_factory_legacy_original() -> Raw {
    LEGACY_ORIGINAL.load(Ordering::Acquire)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_d3d11p_factory_hwnd_original() -> Raw {
    HWND_ORIGINAL.load(Ordering::Acquire)
}

fn hook_targets(factory: Raw) -> (Raw, Raw) {
    let factory2 = unsafe { IDXGIFactory2::from_raw_borrowed(&factory) }.unwrap();
    let factory1 = unsafe { IDXGIFactory::from_raw_borrowed(&factory) }.unwrap();
    (
        factory1.vtable().CreateSwapChain as Raw,
        factory2.vtable().CreateSwapChainForHwnd as Raw,
    )
}

// A refused remove may leave a detour registered. Its original must remain
// available for native replay, and a second installation must leave it alone.
// A target with no registered hook counts as removed.
fn remove_hook(target: Raw, published: &AtomicPtr<c_void>) -> bool {
    if !matches!(
        unsafe { MH_RemoveHook(target) },
        MH_OK | MH_ERROR_NOT_CREATED | MH_ERROR_NOT_INITIALIZED
    ) {
        return false;
    }
    published.store(null_mut(), Ordering::Release);
    true
}

fn install_hooks(factory: Raw) -> u8 {
    if !matches!(
        unsafe { MH_Initialize() },
        MH_OK | MH_ERROR_ALREADY_INITIALIZED
    ) {
        return IDLE;
    }
    let (legacy_target, hwnd_target) = hook_targets(factory);
    let (legacy_detour, hwnd_detour) = (rsf_d3d11p_create_hook as Raw, rsf_d3d11p_hwnd_hook as Raw);
    let mut legacy = null_mut();
    if unsafe { MH_CreateHook(legacy_target, legacy_detour, &mut legacy) } != MH_OK {
        return IDLE;
    }
    let mut hwnd = null_mut();
    if unsafe { MH_CreateHook(hwnd_target, hwnd_detour, &mut hwnd) } != MH_OK {
        return if remove_hook(legacy_target, &LEGACY_ORIGINAL) {
            IDLE
        } else {
            BLOCKED
        };
    }

    // The hook bodies can run as each target is enabled. Until both enables
    // complete they only replay the typed native continuations.
    LEGACY_ORIGINAL.store(legacy, Ordering::Release);
    HWND_ORIGINAL.store(hwnd, Ordering::Release);
    if unsafe { MH_EnableHook(legacy_target) } == MH_OK
        && unsafe { MH_EnableHook(hwnd_target) } == MH_OK
    {
        return READY;
    }

    unsafe { MH_DisableHook(legacy_target) };
    let legacy_removed = remove_hook(legacy_target, &LEGACY_ORIGINAL);
    let hwnd_removed = remove_hook(hwnd_target, &HWND_ORIGINAL);
    if legacy_removed && hwnd_removed {
        IDLE
    } else {
        BLOCKED
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_d3d11_present_install(
    setup: *const rsf_d3d11_present_setup,
) -> i32 {
    unsafe { install(setup, false) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_d3d11_present_install_scoped(
    setup: *const rsf_d3d11_present_setup,
) -> i32 {
    unsafe { install(setup, true) }
}

unsafe fn install(setup: *const rsf_d3d11_present_setup, scoped: bool) -> i32 {
    if HOOK_STATE
        .compare_exchange(IDLE, INSTALLING, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return 0;
    }
    if scoped {
        rsf_core_d3d11p_set_scoped(1);
    }
    // Keep Factory2 live through the install result and its success log, as in
    // the native bootstrap.
    let factory = if unsafe { rsf_core_d3d11p_prepare(setup, 0) } != 0 {
        unsafe { com_ref::owned(rsf_d3d11p_bootstrap_factory()) }
    } else {
        None
    };
    let state = factory
        .as_ref()
        .map_or(IDLE, |factory| install_hooks(factory.as_raw()));
    if state != READY && scoped {
        rsf_core_d3d11p_set_scoped(0);
    }
    HOOK_STATE.store(state, Ordering::Release);
    if state == READY {
        log("presentation bridge: cold-start D3D11 swapchain interception installed");
    } else if state == BLOCKED {
        log("presentation bridge: MinHook cleanup refused; factory interception unavailable");
    }
    i32::from(state == READY)
}

pub(super) type GuardedCreate =
    unsafe extern "C" fn(Raw, Raw, *mut DXGI_SWAP_CHAIN_DESC, *mut Raw) -> i32;

#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn rsf_d3d11p_factory_create_plain(
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
    // The application's fullscreen is a borderless window; bridge chains are always windowed.
    desc.Windowed = BOOL(1);
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
    let chain = unsafe { com_ref::owned(candidate) };
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
    let replacement = unsafe { com_ref::owned(physical) };
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
