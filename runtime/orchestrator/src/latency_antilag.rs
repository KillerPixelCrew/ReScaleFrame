// SPDX-License-Identifier: MIT
use crate::com_ref::vtable;
use crate::sys::{RSF_PACING_NONE, RSF_REFLEX_OFF};
use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Mutex, RwLock};
use windows::{Win32::Graphics::Dxgi::IDXGIObject, core::GUID};

#[path = "latency_antilag_sdk.rs"]
mod sdk;
use sdk::Sdk;

pub(super) const AMD_VENDOR_ID: u32 = 0x1002;

pub(crate) fn fps_from_interval(limit_us: u32) -> u32 {
    1_000_000_u32
        .checked_div(limit_us)
        .map_or(0, |fps| fps.max(1))
}

// The recording driver of the unit tests.
#[cfg(test)]
struct Driver {
    initialize: unsafe extern "C" fn(*mut c_void, u32, *mut c_void) -> u32,
    update: unsafe extern "C" fn(*mut c_void, u32, u32, u32) -> u32,
    end_render: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> u32,
    shutdown: unsafe extern "C" fn(*mut c_void),
}

enum Binding {
    #[cfg(test)]
    Callback(&'static Driver, *mut c_void),
    // Leaked storage. DXGI can keep the embedded DX12 context address after retirement.
    Sdk(*mut Sdk),
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
            #[cfg(test)]
            Self::Callback(calls, raw) => unsafe { (calls.initialize)(raw, api, device) },
            Self::Sdk(raw) => {
                let result = unsafe {
                    match api {
                        11 => Sdk::initialize_dx11(raw),
                        12 => Sdk::initialize_dx12(raw, device),
                        _ => return 0,
                    }
                };
                u32::from(result == 0) // Initialize requires S_OK.
            }
        }
    }

    fn update(&self, api: u32, enabled: u32, fps: u32) -> bool {
        match *self {
            #[cfg(test)]
            Self::Callback(calls, raw) => unsafe { (calls.update)(raw, api, enabled, fps) != 0 },
            Self::Sdk(raw) => {
                let enabled = enabled != 0;
                let result = unsafe {
                    match api {
                        11 => Sdk::update_dx11(raw, enabled, fps),
                        12 => Sdk::update_dx12(raw, enabled, fps),
                        _ => return false,
                    }
                };
                result >= 0
            }
        }
    }

    fn end_render(&self, chain: *mut c_void, attach: u32) -> bool {
        match *self {
            #[cfg(test)]
            Self::Callback(calls, raw) => unsafe { (calls.end_render)(raw, chain, attach) != 0 },
            Self::Sdk(raw) => {
                let data = FrameTypeData {
                    context: unsafe { Sdk::context_dx12(raw) },
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
                tagged.is_ok() && (attach == 0 || unsafe { Sdk::mark_dx12(raw) >= 0 })
            }
        }
    }

    fn shutdown(&self) {
        match *self {
            #[cfg(test)]
            Self::Callback(calls, raw) => unsafe { (calls.shutdown)(raw) },
            Self::Sdk(raw) => unsafe { Sdk::shutdown(raw) },
        }
    }
}

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

/// The D3D owner for AMD hardware. Without a driver the binding reports unavailable on bind.
pub(crate) unsafe extern "C" fn create_sdk_context() -> *mut c_void {
    // Reserve the Rust owner first. DXGI can keep the SDK context address alive after
    // retirement, so the published storage is never freed.
    let Some(storage) = crate::reserve_handle() else {
        return null_mut();
    };
    let binding = Binding::Sdk(Sdk::leak());
    Box::into_raw(Box::write(storage, Context::new(binding))).cast()
}

#[cfg(test)]
mod tests {
    // The Callback binding with a recording driver: the Anti-Lag 2 source, pacing and
    // end-of-render contract.
    use super::*;
    use std::sync::Condvar;
    use std::time::Duration;

    #[derive(Default)]
    struct DriverState {
        initializes: u32,
        updates: u32,
        ends: u32,
        shutdowns: u32,
        api: u32,
        update_api: u32,
        fps: u32,
        enabled: bool,
        attached: bool,
        fail_initialize: bool,
        fail_update: bool,
        sleeping: bool,
    }

    #[derive(Default)]
    struct Fake {
        state: Mutex<DriverState>,
        change: Condvar,
        block_second_update: bool,
    }

    unsafe extern "C" fn initialize(raw: *mut c_void, api: u32, _: *mut c_void) -> u32 {
        let driver = unsafe { &*raw.cast::<Fake>() };
        let mut state = driver.state.lock().unwrap();
        state.initializes += 1;
        state.api = api;
        u32::from(!state.fail_initialize)
    }

    unsafe extern "C" fn update(raw: *mut c_void, api: u32, enabled: u32, fps: u32) -> u32 {
        let driver = unsafe { &*raw.cast::<Fake>() };
        let mut state = driver.state.lock().unwrap();
        state.updates += 1;
        state.update_api = api;
        state.enabled = enabled != 0;
        state.fps = fps;
        if driver.block_second_update && state.updates == 2 {
            state.sleeping = true;
            driver.change.notify_all();
            let (state, _) = driver
                .change
                .wait_timeout_while(state, Duration::from_secs(3), |s| !s.attached)
                .unwrap();
            return u32::from(state.attached);
        }
        u32::from(!state.fail_update)
    }

    unsafe extern "C" fn end_render(raw: *mut c_void, _: *mut c_void, attach: u32) -> u32 {
        let driver = unsafe { &*raw.cast::<Fake>() };
        let mut state = driver.state.lock().unwrap();
        state.ends += 1;
        state.attached = attach != 0;
        driver.change.notify_all();
        1
    }

    unsafe extern "C" fn shutdown(raw: *mut c_void) {
        let driver = unsafe { &*raw.cast::<Fake>() };
        driver.state.lock().unwrap().shutdowns += 1;
    }

    static CALLBACKS: Driver = Driver {
        initialize,
        update,
        end_render,
        shutdown,
    };

    fn fake_context(driver: &Fake) -> Context {
        let raw = std::ptr::from_ref(driver).cast_mut().cast();
        Context::new(Binding::Callback(&CALLBACKS, raw))
    }

    #[test]
    fn callback_driver_follows_the_pacing_contract() {
        const AMD: u32 = 0x1002;
        const NONE: u32 = 0;
        const REFLEX: u32 = 1;
        const XELL: u32 = 2;
        let (mut device, mut chain) = (0u8, 0u8);
        let device: *mut c_void = (&raw mut device).cast();
        let chain: *mut c_void = (&raw mut chain).cast();
        let driver = Fake::default();
        let owner = fake_context(&driver);

        owner.bind(12, device, 0x10de);
        assert_eq!(owner.state().available, 0);
        assert_eq!(driver.state.lock().unwrap().initializes, 0);
        owner.bind(11, device, 0x8086);
        assert_eq!(owner.pace(1, 1, 0, NONE), 0);
        assert_eq!(driver.state.lock().unwrap().updates, 0);
        owner.bind(12, device, AMD);
        assert_eq!(owner.state().available, 1);
        {
            let state = driver.state.lock().unwrap();
            assert_eq!((state.initializes, state.api), (1, 12));
        }
        assert_ne!(owner.pace(1, 1, 10000, NONE), 0);
        assert_eq!(owner.pace(1, 1, 10000, NONE), 0);
        {
            let state = driver.state.lock().unwrap();
            assert!(state.enabled);
            assert_eq!((state.updates, state.fps, state.update_api), (1, 100, 12));
        }
        assert_ne!(owner.end_render(1, chain, 1), 0);
        assert_eq!(owner.end_render(1, chain, 1), 0);
        {
            let state = driver.state.lock().unwrap();
            assert!(state.attached);
            assert_eq!(state.ends, 1);
        }
        assert_ne!(owner.end_render(2, chain, 0), 0);
        assert!(!driver.state.lock().unwrap().attached);
        assert_eq!(owner.pace(2, 0, 0, NONE), 0);
        {
            let state = driver.state.lock().unwrap();
            assert!(!state.enabled && state.fps == 0);
        }
        for (source, mode, pacing) in [(3, 1, XELL), (4, 2, REFLEX)] {
            assert_eq!(owner.pace(source, mode, 0, pacing), 0);
            assert!(!driver.state.lock().unwrap().enabled);
        }
        owner.bind(11, device, AMD);
        assert_eq!(driver.state.lock().unwrap().api, 11);
        assert_ne!(owner.pace(5, 2, 20000, NONE), 0);
        {
            let state = driver.state.lock().unwrap();
            assert_eq!((state.fps, state.update_api), (50, 11));
        }
        assert_eq!(owner.end_render(5, chain, 1), 0);
        driver.state.lock().unwrap().fail_update = true;
        assert_eq!(owner.pace(6, 1, 0, NONE), 0);
        assert_eq!((owner.state().available, owner.state().refused), (0, 1));
        let updates = driver.state.lock().unwrap().updates;
        assert_eq!(owner.pace(7, 1, 0, NONE), 0);
        assert_eq!(driver.state.lock().unwrap().updates, updates);

        let unsupported = Fake::default();
        unsupported.state.lock().unwrap().fail_initialize = true;
        let missing = fake_context(&unsupported);
        missing.bind(12, device, AMD);
        assert_eq!((missing.state().available, missing.state().refused), (0, 1));
        assert_eq!(missing.pace(1, 1, 0, NONE), 0);

        let reset = Fake::default();
        let resetting = fake_context(&reset);
        resetting.bind(12, device, AMD);
        assert_ne!(resetting.pace(1, 1, 0, NONE), 0);
        let before_shutdown = reset.state.lock().unwrap().shutdowns;
        reset.state.lock().unwrap().fail_update = true;
        resetting.reset_sources();
        {
            let state = reset.state.lock().unwrap();
            assert_eq!(
                (state.shutdowns, state.update_api),
                (before_shutdown + 1, 12)
            );
        }
        let state = resetting.state();
        assert_eq!((state.available, state.enabled, state.refused), (0, 0, 1));

        let sleeper = Fake {
            block_second_update: true,
            ..Default::default()
        };
        let concurrent = fake_context(&sleeper);
        concurrent.bind(12, device, AMD);
        assert_ne!(concurrent.pace(1, 1, 0, NONE), 0);
        std::thread::scope(|scope| {
            let pacing = scope.spawn(|| concurrent.pace(2, 1, 0, NONE));
            let state = sleeper.state.lock().unwrap();
            let (state, _) = sleeper
                .change
                .wait_timeout_while(state, Duration::from_secs(2), |s| !s.sleeping)
                .unwrap();
            let sleeping = state.sleeping;
            drop(state);
            assert!(sleeping);
            let mut other = 0u8;
            assert!(!concurrent.bind(12, (&raw mut other).cast(), AMD));
            assert_eq!(concurrent.state().updates, 1);
            assert_ne!(concurrent.end_render(1, chain, 1), 0);
            assert_ne!(pacing.join().unwrap(), 0);
        });
        assert_eq!(
            (concurrent.state().updates, concurrent.state().end_markers),
            (2, 1)
        );
    }
}
