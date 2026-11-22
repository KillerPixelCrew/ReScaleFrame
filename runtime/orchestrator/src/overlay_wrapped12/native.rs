// SPDX-License-Identifier: MIT
use super::*;
use crate::d3d11_d3d12 as graphics;
use crate::sdk_module::SdkModule;
use std::mem::{ManuallyDrop, transmute};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use windows::{
    Win32::Graphics::{
        Direct3D::D3D_FEATURE_LEVEL,
        Direct3D11::{
            D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_BGRA_SUPPORT, ID3D11Device,
            ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource,
        },
        Direct3D11on12::{D3D11_RESOURCE_FLAGS, ID3D11On12Device},
        Direct3D12::{
            D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATES, ID3D12Resource,
        },
        Dxgi::{IDXGISwapChain, IDXGISwapChain3},
    },
    core::{HRESULT, Interface},
};
use windows_sys::Win32::{
    System::{
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        SystemInformation::GetSystemDirectoryW,
    },
    UI::WindowsAndMessaging::IsWindow,
};

const INIT_FAILED: i32 = -6;
type CreateOn12 = unsafe extern "system" fn(
    Handle,
    u32,
    *const D3D_FEATURE_LEVEL,
    u32,
    *const Handle,
    u32,
    u32,
    *mut Option<ID3D11Device>,
    *mut Option<ID3D11DeviceContext>,
    *mut D3D_FEATURE_LEVEL,
) -> HRESULT;

struct SystemEntry {
    _module: SdkModule,
    create: CreateOn12,
}

// The System32 DLL stays loaded for the process and the typed entry is immutable.
unsafe impl Send for SystemEntry {}
unsafe impl Sync for SystemEntry {}

impl SystemEntry {
    fn open() -> Option<Self> {
        let mut path = vec![0; 260];
        let length = unsafe { GetSystemDirectoryW(path.as_mut_ptr(), path.len() as u32) } as usize;
        if length == 0 || length >= path.len() {
            return None;
        }
        path.truncate(length);
        path.extend("\\d3d11.dll".encode_utf16().chain([0]));
        let module = SdkModule::adopt_system(unsafe {
            LoadLibraryExW(path.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
        })?;
        let create = module.symbol(c"D3D11On12CreateDevice")?;
        let create = unsafe {
            transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(
                    *mut std::ffi::c_void,
                    u32,
                    *const windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL,
                    u32,
                    *const *mut std::ffi::c_void,
                    u32,
                    u32,
                    *mut std::option::Option<windows::Win32::Graphics::Direct3D11::ID3D11Device>,
                    *mut std::option::Option<
                        windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
                    >,
                    *mut windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL,
                ) -> windows::core::HRESULT,
            >(create)
        };
        Some(Self {
            _module: module,
            create,
        })
    }
}

unsafe extern "C" fn create_wrapped_device(
    device: Handle,
    queue: Handle,
    out: *mut rsf_overlay11on12_device,
) -> i32 {
    static ENTRY: OnceLock<Option<SystemEntry>> = OnceLock::new();
    let Some(entry) = ENTRY.get_or_init(SystemEntry::open) else {
        return NOT_SUPPORTED;
    };
    let mut device11 = None;
    let mut context11 = None;
    let queues = [queue];
    let result = unsafe {
        (entry.create)(
            device,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT.0,
            null(),
            0,
            queues.as_ptr(),
            queues.len() as u32,
            0,
            &mut device11,
            &mut context11,
            null_mut(),
        )
    };
    let (true, Some(device11), Some(context11)) = (result.is_ok(), device11, context11) else {
        return INIT_FAILED;
    };
    let Ok(interop) = device11.cast::<ID3D11On12Device>() else {
        return INIT_FAILED;
    };
    unsafe {
        out.write(rsf_overlay11on12_device {
            device: device11.into_raw(),
            context: context11.into_raw(),
            interop: interop.into_raw(),
        })
    };
    0
}

unsafe extern "C" fn window(chain: Handle, out: *mut Handle) -> u32 {
    let Ok(desc) = (unsafe { graphics::borrow::<IDXGISwapChain>(&chain).GetDesc() }) else {
        return 0;
    };
    unsafe { out.write(desc.OutputWindow.0) };
    1
}

unsafe fn wrap(
    device: Handle,
    interop: Handle,
    backbuffer: ID3D12Resource,
    state: D3D12_RESOURCE_STATES,
    out: *mut rsf_overlay11on12_target,
) -> Option<()> {
    let device = unsafe { graphics::borrow::<ID3D11Device>(&device) };
    let interop = unsafe { graphics::borrow::<ID3D11On12Device>(&interop) };
    let flags = D3D11_RESOURCE_FLAGS {
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
        ..Default::default()
    };
    let mut wrapped: Option<ID3D11Resource> = None;
    unsafe { interop.CreateWrappedResource(&backbuffer, &flags, state, state, &mut wrapped) }
        .ok()?;
    let wrapped = wrapped?;
    let mut view: Option<ID3D11RenderTargetView> = None;
    unsafe { device.CreateRenderTargetView(&wrapped, None, Some(&mut view)) }.ok()?;
    let view = view?;
    let desc = unsafe { backbuffer.GetDesc() };
    let width = u32::try_from(desc.Width).ok()?;
    if width == 0 || desc.Height == 0 {
        return None;
    }
    unsafe {
        out.write(rsf_overlay11on12_target {
            backbuffer: backbuffer.into_raw(),
            wrapped: wrapped.into_raw(),
            view: view.into_raw(),
            width,
            height: desc.Height,
        })
    };
    Some(())
}

unsafe extern "C" fn target(
    device: Handle,
    interop: Handle,
    chain: Handle,
    out: *mut rsf_overlay11on12_target,
) -> u32 {
    let Ok(chain) =
        (unsafe { graphics::borrow::<IDXGISwapChain>(&chain) }).cast::<IDXGISwapChain3>()
    else {
        return 0;
    };
    let Ok(backbuffer) =
        (unsafe { chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex()) })
    else {
        return 0;
    };
    u32::from(
        unsafe {
            wrap(
                device,
                interop,
                backbuffer,
                D3D12_RESOURCE_STATE_RENDER_TARGET,
                out,
            )
        }
        .is_some(),
    )
}

unsafe extern "C" fn target_present(
    device: Handle,
    interop: Handle,
    backbuffer: Handle,
    out: *mut rsf_overlay11on12_target,
) -> u32 {
    // This is the supplied resource, even if the chain has advanced its index.
    let backbuffer = unsafe { graphics::borrow::<ID3D12Resource>(&backbuffer) }.clone();
    u32::from(
        unsafe {
            wrap(
                device,
                interop,
                backbuffer,
                D3D12_RESOURCE_STATE_PRESENT,
                out,
            )
        }
        .is_some(),
    )
}

unsafe extern "C" fn acquire(interop: Handle, wrapped: Handle) {
    let resources = ManuallyDrop::new([Some(unsafe { ID3D11Resource::from_raw(wrapped) })]);
    unsafe {
        graphics::borrow::<ID3D11On12Device>(&interop).AcquireWrappedResources(&resources[..])
    };
}

unsafe extern "C" fn release(interop: Handle, wrapped: Handle) {
    let resources = ManuallyDrop::new([Some(unsafe { ID3D11Resource::from_raw(wrapped) })]);
    unsafe {
        graphics::borrow::<ID3D11On12Device>(&interop).ReleaseWrappedResources(&resources[..])
    };
}

unsafe extern "C" {
    fn rsf_overlay_host_start_device(
        device: Handle,
        window: Handle,
        log: rsf_overlay12_log,
        user: Handle,
    ) -> i32;
    fn rsf_overlay_host_admit_target(hud: u32, visible: *mut u32, hint: *mut f32) -> i32;
    fn rsf_overlay_host_draw_admitted_target(
        context: Handle,
        view: Handle,
        width: u32,
        height: u32,
        stats: *const rsf_overlay_stats,
        intent: *mut rsf_overlay_intent,
        visible: u32,
        hint: f32,
    ) -> i32;
    fn rsf_overlay_host_stop_device(device: Handle);
}

const PLATFORM: Platform = Platform {
    create: create_wrapped_device,
    window,
    start: rsf_overlay_host_start_device,
    admit: rsf_overlay_host_admit_target,
    target,
    acquire,
    draw: rsf_overlay_host_draw_admitted_target,
    release,
    flush: graphics::flush,
    stop: rsf_overlay_host_stop_device,
};
const PRESENT: Platform = Platform {
    target: target_present,
    ..PLATFORM
};

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_d3d12_frame(
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    unsafe {
        render_frame(
            &PLATFORM,
            device,
            queue,
            chain,
            null_mut(),
            stats,
            intent,
            log,
            user,
        )
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_d3d12_frame_for_window(
    device: Handle,
    queue: Handle,
    backbuffer: Handle,
    window: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    if unsafe { IsWindow(window) } == 0 {
        return -1;
    }
    unsafe {
        render_frame(
            &PRESENT, device, queue, backbuffer, window, stats, intent, log, user,
        )
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_overlay_d3d12_stop() {
    stop(&PLATFORM);
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_d3d12_present(
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: rsf_overlay12_log,
    user: Handle,
) -> i32 {
    unsafe {
        crate::overlay_native12::native::rsf_overlay_native12_present(
            device, queue, chain, stats, intent, log, user,
        )
    }
}
