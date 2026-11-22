// SPDX-License-Identifier: MIT
#[path = "xess_native.rs"]
mod native;

use super::*;
use crate::com_ref::ComRef;
use crate::render_abi::{
    RSF_FG_FIXED, RSF_FG_STAT_ACTIVITY, RSF_FG_STAT_TOTAL_PRESENTED, RSF_PACING_XELL,
    RSF_REFLEX_OFF, rsf_backend_resource as Resource,
};
use std::cell::{Cell, RefCell};
use std::os::windows::io::{AsRawHandle, OwnedHandle};

const REFLEX_ON: u32 = 1;
const INPUT_SAMPLE: u32 = 6;
const FRAME_IDS: usize = 128;

#[repr(C)]
#[derive(Default)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

#[repr(C)]
#[derive(Default)]
struct Present {
    interpolation: i32,
    enabled: u32,
    frames: u32,
}

#[repr(C)]
struct Ops {
    check: &'static Checks,
    resolve: Resolve,
    close: unsafe extern "C" fn(Raw),
    create_fg: unsafe extern "C" fn(Raw, Raw, *mut Raw) -> i32,
    create_xell: unsafe extern "C" fn(Raw, Raw, *mut Raw) -> i32,
    destroy_fg: unsafe extern "C" fn(Raw, Raw) -> i32,
    destroy_xell: unsafe extern "C" fn(Raw, Raw) -> i32,
    max_count: unsafe extern "C" fn(Raw, Raw, *mut u32) -> i32,
    bind_xell: unsafe extern "C" fn(Raw, Raw, Raw) -> i32,
    version: unsafe extern "C" fn(Raw, *mut Version),
    create_chain: unsafe extern "C" fn(Raw, Raw, *const Setup, u32, *mut Raw) -> i32,
    compose: unsafe extern "C" fn(Raw, Raw, u32) -> i32,
    count: unsafe extern "C" fn(Raw, Raw, u32) -> i32,
    enabled: unsafe extern "C" fn(Raw, Raw, u32) -> i32,
    sleep_mode: unsafe extern "C" fn(Raw, Raw, u32, u32) -> i32,
    sleep: unsafe extern "C" fn(Raw, Raw, u32) -> i32,
    marker: unsafe extern "C" fn(Raw, Raw, u32, u32) -> i32,
    present_id: unsafe extern "C" fn(Raw, Raw, u32) -> i32,
    tag: unsafe extern "C" fn(Raw, Raw, Raw, u32, u32, *const Resource) -> i32,
    constants: unsafe extern "C" fn(Raw, Raw, u32, *const Frame, u32) -> i32,
    present_status: unsafe extern "C" fn(Raw, Raw, *mut Present) -> i32,
    create_fence: unsafe extern "C" fn(Raw) -> Raw,
    create_event: unsafe extern "C" fn() -> Raw,
    drain: unsafe extern "C" fn(Raw, Raw, Raw, u64) -> i32,
}

const _: () = {
    assert!(size_of::<Version>() == 12);
    assert!(size_of::<Present>() == 12);
    assert!(size_of::<Ops>() == 24 * size_of::<Raw>());
};

#[derive(Clone, Copy, PartialEq, Eq)]
struct SleepMode {
    interval_us: u32,
    low_latency: bool,
}

impl SleepMode {
    fn requested(options: Options) -> Self {
        Self {
            interval_us: options.frame_limit_us,
            low_latency: options.mode != RSF_FG_OFF || options.reflex_mode != RSF_REFLEX_OFF,
        }
    }
}

#[derive(Default)]
struct SettingsFence {
    fence: Option<ComRef>,
    event: Option<OwnedHandle>,
    serial: u64,
}

impl SettingsFence {
    fn drain(&mut self, ops: &Ops, device: Raw, queue: Raw) -> i32 {
        if self.fence.is_none() {
            self.fence = unsafe { ComRef::from_owned((ops.create_fence)(device)) };
        }
        let Some(fence) = &self.fence else {
            return INIT_FAILED;
        };
        if self.event.is_none() {
            self.event = unsafe { crate::owned_win32_handle((ops.create_event)()) };
        }
        let Some(event) = &self.event else {
            return INIT_FAILED;
        };
        self.serial = self.serial.wrapping_add(1);
        if unsafe { (ops.drain)(queue, fence.as_ptr(), event.as_raw_handle(), self.serial) } >= 0 {
            OK
        } else {
            FEATURE_FAILED
        }
    }
}

#[derive(Clone, Copy)]
struct FrameId {
    sdk: u32,
    input_sampled: bool,
}

struct Latency {
    last_begin: u64,
    sdk_frame: u32,
    ids: Recent<FrameId, FRAME_IDS>,
}

struct Xess {
    ops: &'static Ops,
    sdk: Bindings,
    device: ComRef,
    queue: ComRef,
    log: Log,
    ui_mode: u32,
    state: State,
    fg: Cell<Raw>,
    xell: Cell<Raw>,
    chain: RefCell<Option<ComRef>>,
    reserved: Cell<u32>,
    latency: Mutex<Latency>,
    // Sleep may depend on older frames progressing. Neither their marker
    // lookups nor normal rendering need this guard.
    sleep: Mutex<()>,
    settings_fence: RefCell<SettingsFence>,
    applied_sleep_mode: Cell<Option<SleepMode>>,
    options_dirty: Cell<bool>,
    prepared_reset: Cell<bool>,
    prepared_gap: Cell<u64>,
    last_present_active: Cell<u32>,
}

impl Xess {
    fn chain(&self) -> Raw {
        self.chain
            .borrow()
            .as_ref()
            .map_or(null_mut(), ComRef::as_ptr)
    }

    fn enabled(&self, on: bool) -> bool {
        unsafe { (self.ops.enabled)(self.sdk.raw, self.fg.get(), u32::from(on)) == 0 }
    }

    unsafe fn initialize(&mut self, setup: &Setup) -> i32 {
        let result =
            unsafe { (self.ops.create_fg)(self.sdk.raw, self.device.as_ptr(), self.fg.get_mut()) };
        if result != 0 || self.fg.get().is_null() {
            return UNSUPPORTED;
        }
        let result = unsafe {
            (self.ops.create_xell)(self.sdk.raw, self.device.as_ptr(), self.xell.get_mut())
        };
        if result != 0 || self.xell.get().is_null() {
            return UNSUPPORTED;
        }
        let mut max = 0;
        if unsafe { (self.ops.max_count)(self.sdk.raw, self.fg.get(), &mut max) } != 0
            || max == 0
            || unsafe { (self.ops.bind_xell)(self.sdk.raw, self.fg.get(), self.xell.get()) } != 0
        {
            return UNSUPPORTED;
        }
        let reserved = setup.chain.max_generated_frames.min(max);
        let mut initial = initial_status(reserved);
        initial.pacing_owner = RSF_PACING_XELL;
        initial.low_latency_available = 1;
        let mut version = Version::default();
        unsafe { (self.ops.version)(self.sdk.raw, &mut version) };
        initial.version_id = (u64::from(version.major) << 32)
            | (u64::from(version.minor) << 16)
            | u64::from(version.patch);
        version_name(
            &mut initial,
            &format!("XeFG {}.{}.{}", version.major, version.minor, version.patch),
        );
        if setup.version_id != 0 && setup.version_id != initial.version_id {
            return UNSUPPORTED;
        }
        if !setup.chain.hwnd.is_null() {
            let mut chain = null_mut();
            let result = unsafe {
                (self.ops.create_chain)(self.sdk.raw, self.fg.get(), setup, reserved, &mut chain)
            };
            *self.chain.get_mut() = unsafe { ComRef::from_owned(chain) };
            if result != OK {
                return result;
            }
            if chain.is_null() {
                return INIT_FAILED;
            }
            if unsafe {
                (self.ops.compose)(
                    self.sdk.raw,
                    self.fg.get(),
                    u32::from(self.ui_mode != UI_NONE),
                )
            } != 0
            {
                return FEATURE_FAILED;
            }
        }
        self.reserved.set(reserved);
        self.state = State::new(initial);
        OK
    }
}

impl Session for Xess {
    fn state(&self) -> &State {
        &self.state
    }

    fn configure(&self, options: Options) -> i32 {
        if self.chain().is_null() {
            return NOT_READY;
        }
        if options.mode > RSF_FG_FIXED {
            return UNSUPPORTED;
        }
        let on = options.mode != RSF_FG_OFF;
        let count = options.generated_frames.min(self.reserved.get());
        if on && count == 0 {
            return INVALID;
        }
        let previous = self.state.read().request;
        let reset = self.options_dirty.get()
            || previous.is_none_or(|before| {
                before.mode != options.mode || before.generated_frames != options.generated_frames
            });
        if !reset
            && previous.is_some_and(|before| {
                before.frame_limit_us == options.frame_limit_us
                    && before.reflex_mode == options.reflex_mode
            })
        {
            return OK;
        }
        let requested = SleepMode::requested(options);
        let sleep_changed = self.applied_sleep_mode.get() != Some(requested);
        // Configuration can run on the very frame that releases the next
        // sleep. Let that frame finish and retry a busy settings change.
        let _sleeper = if sleep_changed {
            let Ok(sleeper) = self.sleep.try_lock() else {
                return NOT_READY;
            };
            let result = self.settings_fence.borrow_mut().drain(
                self.ops,
                self.device.as_ptr(),
                self.queue.as_ptr(),
            );
            if result != OK {
                return result;
            }
            Some(sleeper)
        } else {
            None
        };
        self.options_dirty.set(true);
        if count != 0 && unsafe { (self.ops.count)(self.sdk.raw, self.fg.get(), count) } != 0 {
            return FEATURE_FAILED;
        }
        if sleep_changed {
            if unsafe {
                (self.ops.sleep_mode)(
                    self.sdk.raw,
                    self.xell.get(),
                    requested.interval_us,
                    u32::from(requested.low_latency),
                )
            } != 0
            {
                return FEATURE_FAILED;
            }
            // SetSleepMode succeeded even if the following FG call fails.
            // A later rollback must compare against the mode XeLL actually has.
            self.applied_sleep_mode.set(Some(requested));
        }
        if !self.enabled(on) {
            return FEATURE_FAILED;
        }
        self.options_dirty.set(false);
        self.state.configured(
            options,
            options.mode,
            if on { count } else { 0 },
            if requested.low_latency {
                REFLEX_ON
            } else {
                RSF_REFLEX_OFF
            },
            reset,
        );
        OK
    }

    fn begin(&self, id: u64) -> i32 {
        let _sleeper = self.sleep.lock().unwrap();
        let sdk = {
            let mut latency = self.latency.lock().unwrap();
            if id <= latency.last_begin {
                return INVALID;
            }
            let Some(next) = latency.sdk_frame.checked_add(1) else {
                return NEEDS_RESTART;
            };
            latency.sdk_frame = next;
            next
        };
        if unsafe { (self.ops.sleep)(self.sdk.raw, self.xell.get(), sdk) } != 0 {
            return FEATURE_FAILED;
        }
        let mut latency = self.latency.lock().unwrap();
        latency.last_begin = id;
        latency.ids.insert(
            id,
            FrameId {
                sdk,
                input_sampled: false,
            },
        );
        self.state.paced();
        OK
    }

    fn marker(&self, kind: u32, id: u64) -> i32 {
        if kind > INPUT_SAMPLE {
            return INVALID;
        }
        let mut latency = self.latency.lock().unwrap();
        let Some(frame) = latency.ids.get(id) else {
            return NOT_READY;
        };
        if kind == INPUT_SAMPLE {
            if frame.input_sampled {
                return OK;
            }
            frame.input_sampled = true;
        }
        if unsafe { (self.ops.marker)(self.sdk.raw, self.xell.get(), frame.sdk, kind) } == 0 {
            OK
        } else {
            FEATURE_FAILED
        }
    }

    unsafe fn prepare(&self, list: Raw, frame: *const Frame) -> i32 {
        let (frame, record) =
            match unsafe { read_frame(self.ops.check, frame, self.device.as_ptr()) } {
                Ok(frame) => frame,
                Err(result) => return result,
            };
        if record.source_provenance == rsf_game_sys::RSF_FRAME_SOURCE_RENDERED {
            return UNSUPPORTED;
        }
        let before = self.state.read();
        let id = record.frame_id;
        if self.chain().is_null() || id <= before.last_prepare {
            return NOT_READY;
        }
        let sdk = self
            .latency
            .lock()
            .unwrap()
            .ids
            .get(id)
            .map(|frame| frame.sdk);
        let Some(sdk) = sdk else {
            return NOT_READY;
        };
        let fg = self.fg.get();
        if unsafe { (self.ops.present_id)(self.sdk.raw, fg, sdk) } != 0 {
            return FEATURE_FAILED;
        }
        if frame.interpolate == 0 || before.mode() == RSF_FG_OFF {
            let staged = self.state.stage(before.revision, id, RSF_FG_OFF, 0);
            let result = configure_disabled(&self.state, before, id, staged);
            return if self.enabled(false) {
                result
            } else {
                FEATURE_FAILED
            };
        }
        let result = unsafe { (self.ops.check.command)(list, self.device.as_ptr()) };
        if result != OK {
            return result;
        }
        if (self.ui_mode != UI_NONE && frame.hudless.resource.is_null())
            || (self.ui_mode == HUDLESS_UI && frame.ui.resource.is_null())
        {
            return INVALID;
        }
        if !self.state.stage(
            before.revision,
            id,
            before.status.effective_mode,
            before.status.effective_generated_frames,
        ) {
            return NOT_READY;
        }
        let reset = before.reset_for(record);
        let resources = [&frame.depth, &frame.motion, &frame.hudless, &frame.ui];
        for (kind, resource) in resources.into_iter().enumerate() {
            if !resource.resource.is_null()
                && unsafe { (self.ops.tag)(self.sdk.raw, fg, list, sdk, kind as u32, resource) }
                    != 0
            {
                return FEATURE_FAILED;
            }
        }
        if unsafe { (self.ops.constants)(self.sdk.raw, fg, sdk, frame, u32::from(reset)) } != 0
            || !self.enabled(true)
        {
            return FEATURE_FAILED;
        }
        self.prepared_reset.set(reset);
        self.prepared_gap.set(id - before.last_prepare);
        let count = before
            .request
            .map_or(0, |options| options.generated_frames)
            .min(self.reserved.get());
        if self
            .state
            .complete(before.revision, id, true, before.mode(), count)
        {
            OK
        } else {
            NOT_READY
        }
    }

    fn after(&self) -> i32 {
        let before = self.state.read();
        let mut present = Present::default();
        let result =
            unsafe { (self.ops.present_status)(self.sdk.raw, self.fg.get(), &mut present) };
        let mut observation = Observation {
            vendor_status: result,
            ..Observation::default()
        };
        if result == 0 {
            observation.vendor_status = present.interpolation;
            observation.active =
                present.frames > 1 && present.enabled != 0 && present.interpolation == 0;
            observation.presented = u64::from(present.frames);
            observation.statistics = RSF_FG_STAT_TOTAL_PRESENTED | RSF_FG_STAT_ACTIVITY;
        }
        let status = self.state.observe(before.revision, observation);
        let previous_active = self.last_present_active.replace(status.active);
        if previous_active != status.active || before.status.vendor_status != status.vendor_status {
            self.log.write(format_args!(
                "XeSS FG present source={} query={result} interpolation={} enabled={} presented={} active={} reset={} source_gap={}",
                before.last_prepare, present.interpolation, present.enabled, present.frames, status.active,
                u32::from(self.prepared_reset.get()), self.prepared_gap.get()
            ));
        }
        if result < 0 { FEATURE_FAILED } else { OK }
    }

    fn retire(&self) -> bool {
        let chain = self.chain.borrow_mut().take();
        drop(chain);
        let fg = self.fg.get();
        if !fg.is_null() && unsafe { (self.ops.destroy_fg)(self.sdk.raw, fg) } != 0 {
            return false;
        }
        self.fg.set(null_mut());
        let xell = self.xell.get();
        if !xell.is_null() && unsafe { (self.ops.destroy_xell)(self.sdk.raw, xell) } != 0 {
            return false;
        }
        self.xell.set(null_mut());
        true
    }
}

static PROVIDER: Provider = provider::<Xess>();

#[unsafe(no_mangle)]
extern "C" fn rsf_core_xeg12_provider() -> *const Provider {
    &PROVIDER
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_xeg12_create(
    ops: &'static Ops,
    setup: *const Setup,
    out: *mut Raw,
    chain: *mut Raw,
) -> i32 {
    let result = unsafe { (ops.check.setup)(setup, out, chain) };
    if result != OK {
        return result;
    }
    let setup = unsafe { &*setup };
    if setup.chain.ui_mode == UI_LAYER {
        return UNSUPPORTED;
    }
    if setup.streamline_host.is_null() && unsafe { (ops.check.streamline_loaded)() } != 0 {
        return NEEDS_RESTART;
    }
    let sdk = match Bindings::open(
        setup,
        ops.check,
        "libxess_fg.dll",
        Some("libxell.dll"),
        ops.resolve,
        ops.close,
    ) {
        Ok(sdk) => sdk,
        Err(result) => return result,
    };
    let session = crate::allocate_handle(Xess {
        ops,
        sdk,
        log: Log::new(setup),
        device: unsafe { ComRef::retain(setup.chain.d3d12_device) },
        queue: unsafe { ComRef::retain(setup.chain.d3d12_queue) },
        ui_mode: setup.chain.ui_mode,
        state: State::new(initial_status(0)),
        fg: Cell::new(null_mut()),
        xell: Cell::new(null_mut()),
        chain: RefCell::new(None),
        reserved: Cell::new(0),
        latency: Mutex::new(Latency {
            last_begin: 0,
            sdk_frame: 0,
            ids: Recent::new(),
        }),
        sleep: Mutex::new(()),
        settings_fence: RefCell::new(SettingsFence::default()),
        applied_sleep_mode: Cell::new(None),
        options_dirty: Cell::new(false),
        prepared_reset: Cell::new(false),
        prepared_gap: Cell::new(0),
        last_present_active: Cell::new(0),
    });
    let Some(owner) = (unsafe { session.as_mut() }) else {
        return INIT_FAILED;
    };
    let result = unsafe { owner.initialize(setup) };
    if result != OK {
        unsafe { destroy::<Xess>(session.cast()) };
        return result;
    }
    unsafe {
        out.write(session.cast());
        chain.write(owner.chain());
    }
    OK
}
