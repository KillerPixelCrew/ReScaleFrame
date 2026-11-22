// SPDX-License-Identifier: MIT
use crate::com_ref::vtable;
use crate::render_abi::{RSF_PACING_NONE, RSF_REFLEX_OFF};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Mutex, RwLock};
use windows::{Win32::Graphics::Dxgi::IDXGIObject, core::GUID};

#[path = "latency_antilag_native_abi.rs"]
mod native_abi;
use native_abi::AntiLagNativeOps;

pub(super) const AMD_VENDOR_ID: u32 = 0x1002;

pub(crate) fn fps_from_interval(limit_us: u32) -> u32 {
    1_000_000_u32
        .checked_div(limit_us)
        .map_or(0, |fps| fps.max(1))
}

#[repr(C)]
struct Driver {
    initialize: unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> u32,
    update: unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> u32,
    end_render: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> u32,
    shutdown: unsafe extern "C" fn(*mut c_void),
}

const _: () = assert!(size_of::<Driver>() == 32);

enum Binding {
    Unavailable,
    Callback(&'static Driver, *mut c_void),
    Sdk(&'static AntiLagNativeOps, *mut c_void),
}

// The pinned FSR DX12 swap chain reads a Context* followed by a C++ bool.
// Initialize the padding too, since DXGI copies the entire x64 C record.
#[repr(C)]
struct FrameTypeData {
    context: *mut c_void,
    enabled: u8,
    padding: [u8; 7],
}

const _: () = {
    assert!(size_of::<FrameTypeData>() == 16);
    assert!(std::mem::offset_of!(FrameTypeData, enabled) == 8);
};
const FRAME_TYPE_KEY: GUID = GUID::from_u128(0x5083ae5b_8070_4fca_8ee5_3582dd367d13);

impl Binding {
    fn initialize(&self, api: u32, device: *mut c_void) -> u32 {
        match *self {
            Self::Unavailable => 0,
            Self::Callback(calls, raw) => unsafe { (calls.initialize)(raw, api, device) },
            Self::Sdk(ops, raw) => {
                let result = unsafe {
                    match api {
                        11 => (ops.initialize_dx11.unwrap())(raw),
                        12 => (ops.initialize_dx12.unwrap())(raw, device),
                        _ => return 0,
                    }
                };
                u32::from(result == 0) // Initialize requires S_OK.
            }
        }
    }

    fn update(&self, api: u32, enabled: u32, fps: u32) -> bool {
        match *self {
            Self::Unavailable => false,
            Self::Callback(calls, raw) => unsafe { (calls.update)(raw, api, enabled, fps) != 0 },
            Self::Sdk(ops, raw) => {
                let update = match api {
                    11 => ops.update_dx11,
                    12 => ops.update_dx12,
                    _ => return false,
                };
                unsafe { update.unwrap()(raw, enabled, fps) >= 0 }
            }
        }
    }

    fn end_render(&self, chain: *mut c_void, attach: u32) -> bool {
        match *self {
            Self::Unavailable => false,
            Self::Callback(calls, raw) => unsafe { (calls.end_render)(raw, chain, attach) != 0 },
            Self::Sdk(ops, raw) => {
                let data = FrameTypeData {
                    context: unsafe { (ops.context_dx12.unwrap())(raw) },
                    enabled: u8::from(attach != 0),
                    padding: [0; 7],
                };
                // The live swap chain is borrowed from the Present caller.
                // SetPrivateData is inherited from IDXGIObject.
                let tagged = unsafe {
                    (vtable::<IDXGIObject>(&chain).SetPrivateData)(
                        chain,
                        &FRAME_TYPE_KEY,
                        size_of::<FrameTypeData>() as u32,
                        std::ptr::from_ref(&data).cast(),
                    )
                };
                tagged.is_ok() && (attach == 0 || unsafe { (ops.mark_dx12.unwrap())(raw) >= 0 })
            }
        }
    }

    fn shutdown(&self) {
        match *self {
            Self::Unavailable => {}
            Self::Callback(calls, raw) => unsafe { (calls.shutdown)(raw) },
            Self::Sdk(ops, raw) => unsafe { (ops.shutdown.unwrap())(raw) },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct State {
    pub(crate) available: u32,
    pub(crate) enabled: u32,
    pub(crate) updates: u64,
    end_markers: u64,
    pub(crate) refused: u64,
}

#[derive(Default)]
struct Sources {
    api: u32,
    device: usize,
    vendor: u32,
    last_update: u64,
    last_end: u64,
    status: State,
}

pub(crate) struct Context {
    driver: Binding,
    lifetime: RwLock<()>,
    updater: Mutex<()>,
    sources: Mutex<Sources>,
}

// The caller keeps the driver alive. A shared lifetime lease permits DX12
// Update and the older frame's EndOfFrameRendering call to overlap. Only
// Update touches the SDK context's settings fields. Initialize and shutdown
// require the exclusive lease, and updates serialize independently.
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    fn new(driver: Binding) -> Self {
        Self {
            driver,
            lifetime: RwLock::new(()),
            updater: Mutex::new(()),
            sources: Mutex::new(Sources::default()),
        }
    }

    pub(crate) fn bind(&self, api: u32, device: *mut c_void, vendor: u32) -> bool {
        let binding = (api, device as usize, vendor);
        let current = |s: &Sources| (s.api, s.device, s.vendor);
        if current(&self.sources.lock().unwrap()) == binding {
            return true;
        }
        let Ok(_update) = self.updater.try_lock() else {
            return false;
        };
        let Ok(_driver) = self.lifetime.try_write() else {
            return false;
        };
        let mut sources = self.sources.lock().unwrap();
        if current(&sources) == binding {
            return true;
        }
        self.driver.shutdown();
        (sources.api, sources.device, sources.vendor) = binding;
        sources.last_update = 0;
        sources.last_end = 0;
        sources.status.available = 0;
        sources.status.enabled = 0;
        if vendor == AMD_VENDOR_ID && !device.is_null() && matches!(api, 11 | 12) {
            sources.status.available = self.driver.initialize(api, device);
            if sources.status.available == 0 {
                sources.status.refused = sources.status.refused.wrapping_add(1);
            }
        }
        true
    }

    pub(crate) fn pace(&self, id: u64, mode: u32, limit_us: u32, other_owner: u32) -> u32 {
        let _update = self.updater.lock().unwrap();
        let driver = self.lifetime.read().unwrap();
        let api = {
            let mut sources = self.sources.lock().unwrap();
            if sources.status.available == 0 || id == 0 || id <= sources.last_update {
                return 0;
            }
            sources.last_update = id;
            sources.api
        };
        let enabled = mode != RSF_REFLEX_OFF && other_owner == RSF_PACING_NONE;
        let fps = if enabled {
            fps_from_interval(limit_us)
        } else {
            0
        };
        let accepted = self.driver.update(api, u32::from(enabled), fps);
        let mut sources = self.sources.lock().unwrap();
        if accepted {
            sources.status.enabled = u32::from(enabled);
            sources.status.updates = sources.status.updates.wrapping_add(1);
            return sources.status.enabled;
        }
        sources.status.refused = sources.status.refused.wrapping_add(1);
        sources.status.available = 0;
        sources.status.enabled = 0;
        drop(sources);
        drop(driver);
        let _driver = self.lifetime.write().unwrap();
        self.driver.shutdown();
        0
    }

    pub(crate) fn end_render(&self, id: u64, chain: *mut c_void, generated: u32) -> u32 {
        let _driver = self.lifetime.read().unwrap();
        let attach = {
            let mut sources = self.sources.lock().unwrap();
            if sources.status.available == 0
                || sources.api != 12
                || chain.is_null()
                || id == 0
                || id == sources.last_end
            {
                return 0;
            }
            sources.last_end = id;
            u32::from(sources.status.enabled != 0 && generated != 0)
        };
        let accepted = self.driver.end_render(chain, attach);
        let mut sources = self.sources.lock().unwrap();
        if !accepted {
            sources.status.refused = sources.status.refused.wrapping_add(1);
            sources.status.enabled = 0;
            return 0;
        }
        sources.status.end_markers = sources.status.end_markers.wrapping_add(1);
        1
    }

    pub(crate) fn state(&self) -> State {
        self.sources.lock().unwrap().status
    }

    pub(crate) fn reset_sources(&self) {
        let _update = self.updater.lock().unwrap();
        let _driver = self.lifetime.write().unwrap();
        let mut sources = self.sources.lock().unwrap();
        if sources.status.available != 0
            && sources.status.enabled != 0
            && !self.driver.update(sources.api, 0, 0)
        {
            sources.status.refused = sources.status.refused.wrapping_add(1);
            sources.status.available = 0;
            self.driver.shutdown();
        }
        sources.status.enabled = 0;
        sources.last_update = 0;
        sources.last_end = 0;
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        self.driver.shutdown();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_antilag_create(
    calls: Option<&'static Driver>,
    driver: *mut c_void,
) -> *mut Context {
    let binding = calls.map_or(Binding::Unavailable, |calls| {
        Binding::Callback(calls, driver)
    });
    crate::allocate_handle(Context::new(binding))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_antilag_create_sdk(ops: Option<&'static AntiLagNativeOps>) -> *mut Context {
    let Some(ops) = ops else {
        return crate::allocate_handle(Context::new(Binding::Unavailable));
    };
    // Reserve the Rust owner first. DXGI can keep the embedded SDK context
    // address alive after retirement, so published native storage is never freed.
    let Some(storage) = crate::reserve_handle() else {
        return null_mut();
    };
    let raw = unsafe { (ops.allocate.unwrap())() };
    if raw.is_null() {
        return null_mut();
    }
    Box::into_raw(Box::write(storage, Context::new(Binding::Sdk(ops, raw))))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_destroy(context: *mut Context) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context) });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_bind(
    context: *mut Context,
    api: u32,
    device: *mut c_void,
    vendor: u32,
) -> u32 {
    u32::from(unsafe { context.as_ref() }.is_some_and(|context| context.bind(api, device, vendor)))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_pace(
    context: *mut Context,
    id: u64,
    mode: u32,
    limit_us: u32,
    other_owner: u32,
) -> u32 {
    unsafe { context.as_ref() }.map_or(0, |c| c.pace(id, mode, limit_us, other_owner))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_end_render(
    context: *mut Context,
    id: u64,
    chain: *mut c_void,
    generated: u32,
) -> u32 {
    unsafe { context.as_ref() }.map_or(0, |c| c.end_render(id, chain, generated))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_state(context: *const Context, out: *mut State) {
    let state = unsafe { context.as_ref() }
        .map(Context::state)
        .unwrap_or_default();
    unsafe { out.write(state) };
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_antilag_reset_sources(context: *mut Context) {
    if let Some(context) = unsafe { context.as_ref() } {
        context.reset_sources();
    }
}
