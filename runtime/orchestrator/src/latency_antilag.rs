// SPDX-License-Identifier: MIT
use crate::render_abi::{RSF_PACING_NONE, RSF_REFLEX_OFF};
use std::ffi::c_void;
use std::sync::{Mutex, RwLock};

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
    calls: &'static Driver,
    driver: *mut c_void,
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
        unsafe { (self.calls.shutdown)(self.driver) };
        (sources.api, sources.device, sources.vendor) = binding;
        sources.last_update = 0;
        sources.last_end = 0;
        sources.status.available = 0;
        sources.status.enabled = 0;
        if vendor == AMD_VENDOR_ID && !device.is_null() && matches!(api, 11 | 12) {
            sources.status.available = unsafe { (self.calls.initialize)(self.driver, api, device) };
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
        let accepted =
            unsafe { (self.calls.update)(self.driver, api, u32::from(enabled), fps) } != 0;
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
        unsafe { (self.calls.shutdown)(self.driver) };
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
        let accepted = unsafe { (self.calls.end_render)(self.driver, chain, attach) };
        let mut sources = self.sources.lock().unwrap();
        if accepted == 0 {
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
            && unsafe { (self.calls.update)(self.driver, sources.api, 0, 0) } == 0
        {
            sources.status.refused = sources.status.refused.wrapping_add(1);
            sources.status.available = 0;
            unsafe { (self.calls.shutdown)(self.driver) };
        }
        sources.status.enabled = 0;
        sources.last_update = 0;
        sources.last_end = 0;
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe { (self.calls.shutdown)(self.driver) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_antilag_create(calls: &'static Driver, driver: *mut c_void) -> *mut Context {
    crate::allocate_handle(Context {
        calls,
        driver,
        lifetime: RwLock::new(()),
        updater: Mutex::new(()),
        sources: Mutex::new(Sources::default()),
    })
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
