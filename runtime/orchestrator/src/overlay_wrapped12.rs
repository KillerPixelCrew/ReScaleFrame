// SPDX-License-Identifier: MIT
//! The D3D11-on-12 panel (overlay_d3d12.cpp): the shared D3D11 overlay host draws into a D3D12
//! back buffer through a D3D11-on-12 device created on the presenting queue.
#![cfg_attr(test, allow(dead_code))]
use crate::overlay_host::{
    rsf_core_overlay_host_admit, rsf_core_overlay_host_draw_target,
    rsf_core_overlay_host_start_device, rsf_core_overlay_host_stop_device,
};
use crate::overlay_native12::OverlayLog;
use crate::sdk_module::SdkModule;
use crate::sr_session::has_capacity;
use crate::sys::{rsf_overlay_intent, rsf_overlay_stats};
use std::ffi::{CStr, c_void};
use std::mem::{ManuallyDrop, transmute};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock};
use windows::Win32::Graphics::{
    Direct3D::D3D_FEATURE_LEVEL,
    Direct3D11::{
        D3D11_BIND_RENDER_TARGET, D3D11_CREATE_DEVICE_BGRA_SUPPORT, ID3D11Device,
        ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource,
    },
    Direct3D11on12::{D3D11_RESOURCE_FLAGS, ID3D11On12Device},
    Direct3D12::{
        D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATES,
        ID3D12CommandQueue, ID3D12Device, ID3D12Resource,
    },
    Dxgi::{IDXGISwapChain, IDXGISwapChain3},
};
use windows::core::{HRESULT, Interface};
use windows_sys::Win32::{
    System::{
        LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        SystemInformation::GetSystemDirectoryW,
    },
    UI::WindowsAndMessaging::IsWindow,
};

type Handle = *mut c_void;

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
    // Resolved only when this optional path is used: local D3D11 wrappers may expose device
    // creation without the D3D11-on-12 entry.
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
        let create =
            unsafe { transmute::<unsafe extern "system" fn() -> isize, CreateOn12>(create) };
        Some(Self {
            _module: module,
            create,
        })
    }
}

struct Binding {
    interop: ID3D11On12Device,
    context: ID3D11DeviceContext,
    device: ID3D11Device,
    owner: ID3D12Device,
    queue: ID3D12CommandQueue,
    started: bool,
}

// These COM references move between calls. The owner lock serializes immediate-context work and
// teardown, and the D3D12 queue remains the one supplied at creation.
unsafe impl Send for Binding {}

impl Binding {
    fn create(
        create: CreateOn12,
        owner: &ID3D12Device,
        queue: &ID3D12CommandQueue,
    ) -> Option<Self> {
        let mut device = None;
        let mut context = None;
        let queues = [queue.as_raw()];
        let result = unsafe {
            create(
                owner.as_raw(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT.0,
                null(),
                0,
                queues.as_ptr(),
                queues.len() as u32,
                0,
                &mut device,
                &mut context,
                null_mut(),
            )
        };
        let (true, Some(device), Some(context)) = (result.is_ok(), device, context) else {
            return None;
        };
        Some(Self {
            interop: device.cast().ok()?,
            context,
            device,
            owner: owner.clone(),
            queue: queue.clone(),
            started: false,
        })
    }

    // The wrapped target and its view over `backbuffer`, which is in `state` at this Present.
    fn wrap(
        &self,
        backbuffer: &ID3D12Resource,
        state: D3D12_RESOURCE_STATES,
    ) -> Option<(ID3D11Resource, ID3D11RenderTargetView, u32, u32)> {
        let flags = D3D11_RESOURCE_FLAGS {
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            ..Default::default()
        };
        let mut wrapped: Option<ID3D11Resource> = None;
        unsafe {
            self.interop
                .CreateWrappedResource(backbuffer, &flags, state, state, &mut wrapped)
        }
        .ok()?;
        let wrapped = wrapped?;
        let mut view = None;
        unsafe {
            self.device
                .CreateRenderTargetView(&wrapped, None, Some(&mut view))
        }
        .ok()?;
        let desc = unsafe { backbuffer.GetDesc() };
        let width = u32::try_from(desc.Width).ok()?;
        if width == 0 || desc.Height == 0 {
            return None;
        }
        Some((wrapped, view?, width, desc.Height))
    }
}

// What the panel draws into: the chain's current buffer, or one supplied with its window.
enum Target<'a> {
    Chain(&'a IDXGISwapChain),
    Buffer {
        backbuffer: &'a ID3D12Resource,
        window: Handle,
    },
}

struct Frame<'a> {
    device: &'a ID3D12Device,
    queue: &'a ID3D12CommandQueue,
    target: Target<'a>,
    stats: &'a rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: OverlayLog,
    user: Handle,
}

impl Frame<'_> {
    fn log(&self, message: &CStr) {
        unsafe {
            crate::graphics_d3d11::rsf_call_native_log(self.logger, self.user, message.as_ptr())
        };
    }
}

#[derive(Default)]
struct State {
    binding: Option<Binding>,
    // A refused start is final until stop: retrying each frame would reload the panel library.
    failed: bool,
}

static OWNER: Mutex<State> = Mutex::new(State {
    binding: None,
    failed: false,
});

impl State {
    fn refuse(&mut self, frame: &Frame<'_>, message: &CStr) -> i32 {
        self.failed = true;
        frame.log(message);
        0
    }

    fn render(&mut self, frame: &Frame<'_>) -> i32 {
        static ENTRY: OnceLock<Option<SystemEntry>> = OnceLock::new();
        if self.failed {
            return 0;
        }
        if self.binding.is_none() {
            let Some(entry) = ENTRY.get_or_init(SystemEntry::open) else {
                return self.refuse(
                    frame,
                    c"Shared overlay unavailable: system D3D11-on-12 entry point not available.",
                );
            };
            let Some(binding) = Binding::create(entry.create, frame.device, frame.queue) else {
                return self.refuse(
                    frame,
                    c"Shared overlay unavailable: the D3D11-on-12 device could not be created.",
                );
            };
            self.binding = Some(binding);
        }
        let binding = self.binding.as_mut().unwrap();
        if binding.owner.as_raw() != frame.device.as_raw()
            || binding.queue.as_raw() != frame.queue.as_raw()
        {
            return 0;
        }
        if !binding.started {
            let window = match frame.target {
                Target::Chain(chain) => match unsafe { chain.GetDesc() } {
                    Ok(desc) => desc.OutputWindow.0,
                    Err(_) => return 0,
                },
                Target::Buffer { window, .. } => window,
            };
            let user = frame.user;
            if unsafe {
                rsf_core_overlay_host_start_device(
                    binding.device.as_raw(),
                    window,
                    frame.logger,
                    user,
                )
            } == 0
            {
                return self.refuse(
                    frame,
                    c"Shared overlay did not start; it will not be retried.",
                );
            }
            binding.started = true;
            frame.log(c"Shared overlay attached to D3D12 queue and window; Insert opens presets.");
        }
        // Hidden, no HUD and past the startup hint: skip the wrap, view, acquire, release and flush.
        let Some(admission) = rsf_core_overlay_host_admit(frame.stats.show_performance_hud != 0)
        else {
            return 0;
        };
        let (backbuffer, state) = match frame.target {
            Target::Chain(chain) => {
                let Ok(chain) = chain.cast::<IDXGISwapChain3>() else {
                    return 0;
                };
                let Ok(backbuffer) = (unsafe {
                    chain.GetBuffer::<ID3D12Resource>(chain.GetCurrentBackBufferIndex())
                }) else {
                    return 0;
                };
                (backbuffer, D3D12_RESOURCE_STATE_RENDER_TARGET)
            }
            // This is the supplied resource, even if the chain has advanced its index.
            Target::Buffer { backbuffer, .. } => (backbuffer.clone(), D3D12_RESOURCE_STATE_PRESENT),
        };
        let Some((wrapped, view, width, height)) = binding.wrap(&backbuffer, state) else {
            return 0;
        };
        let acquired =
            ManuallyDrop::new([Some(unsafe { ID3D11Resource::from_raw(wrapped.as_raw()) })]);
        unsafe { binding.interop.AcquireWrappedResources(&acquired[..]) };
        let drawn = unsafe {
            rsf_core_overlay_host_draw_target(
                binding.context.as_raw(),
                view.as_raw(),
                width,
                height,
                frame.stats,
                frame.intent,
                Some(admission),
            )
        };
        unsafe { binding.interop.ReleaseWrappedResources(&acquired[..]) };
        // Release the D3D11 references first. Flush also retires their deferred destruction, so
        // they do not hold the back buffer across ResizeBuffers.
        drop(view);
        drop(wrapped);
        unsafe { binding.context.Flush() };
        drawn
    }

    fn stop(&mut self) {
        if let Some(binding) = &self.binding
            && binding.started
        {
            rsf_core_overlay_host_stop_device(binding.device.as_raw());
        }
        *self = Self::default();
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn render_frame(
    device: Handle,
    queue: Handle,
    target: Option<Target<'_>>,
    window_owned: bool,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    logger: OverlayLog,
    user: Handle,
) -> i32 {
    let (Some(device), Some(queue), Some(target)) = (
        unsafe { ID3D12Device::from_raw_borrowed(&device) },
        unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) },
        target,
    ) else {
        return 0;
    };
    if !unsafe { has_capacity(stats) } || (!intent.is_null() && !unsafe { has_capacity(intent) }) {
        return 0;
    }
    let Ok(mut owner) = OWNER.try_lock() else {
        return 0;
    };
    let drawn = owner.render(&Frame {
        device,
        queue,
        target,
        stats: unsafe { &*stats },
        intent,
        logger,
        user,
    });
    if window_owned && owner.failed {
        -1
    } else {
        drawn
    }
}

#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_overlay_d3d12_frame(
    device: Handle,
    queue: Handle,
    chain: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: OverlayLog,
    user: Handle,
) -> i32 {
    // The chain is borrowed for this call only.
    let target = unsafe { IDXGISwapChain::from_raw_borrowed(&chain) }.map(Target::Chain);
    unsafe { render_frame(device, queue, target, false, stats, intent, log, user) }
}

#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn rsf_overlay_d3d12_frame_for_window(
    device: Handle,
    queue: Handle,
    backbuffer: Handle,
    window: Handle,
    stats: *const rsf_overlay_stats,
    intent: *mut rsf_overlay_intent,
    log: OverlayLog,
    user: Handle,
) -> i32 {
    if unsafe { IsWindow(window) } == 0 {
        return -1;
    }
    let target = unsafe { ID3D12Resource::from_raw_borrowed(&backbuffer) }
        .map(|backbuffer| Target::Buffer { backbuffer, window });
    unsafe { render_frame(device, queue, target, true, stats, intent, log, user) }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_overlay_d3d12_stop() {
    OWNER.lock().unwrap().stop();
}
