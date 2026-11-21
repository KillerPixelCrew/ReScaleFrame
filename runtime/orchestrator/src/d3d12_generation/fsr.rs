// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref::ComRef;
use crate::frame_capture12::{Owner as Capture, Platform as CapturePlatform};
use crate::render_abi::{
    RSF_FG_FIXED, RSF_FG_STAT_ACTIVITY, RSF_FG_STAT_PRESENT_CALLBACKS, RSF_FG_STAT_TOTAL_PRESENTED,
    RSF_REFLEX_OFF, rsf_backend_resource as Resource,
};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::Relaxed};

const CAPTURE_SLOTS: usize = 64;
const NON_PIXEL_READ: u32 = 0x40; // D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
const VENDOR_PARAMETER_ERROR: u32 = 6;

#[repr(C)]
struct Configuration {
    chain: Raw,
    user: Raw,
    hudless: Raw,
    sequence: u64,
    width: u32,
    height: u32,
    enabled: u32,
    copy_present: u32,
}

#[repr(C)]
#[derive(Default)]
struct CapturedResource {
    resource: Raw,
    state: u32,
}

#[repr(C)]
#[derive(Default)]
struct Generated {
    command: Raw,
    sequence: u64,
    count: u32,
    reset: u32,
    input: CapturedResource,
    outputs: [CapturedResource; 4],
}

#[repr(C)]
struct Ops {
    check: &'static Checks,
    capture: &'static CapturePlatform,
    open: unsafe extern "C" fn(*const Setup, *mut Raw) -> i32,
    close: unsafe extern "C" fn(Raw),
    select_version: unsafe extern "C" fn(Raw, *const Setup, *mut c_char, u32) -> u64,
    surface_format: unsafe extern "C" fn(u32) -> u32,
    create_generation:
        unsafe extern "C" fn(Raw, *const Extent, *const Status, *const c_char, *mut Raw) -> i32,
    provider_matches: unsafe extern "C" fn(Raw, Raw, u64) -> i32,
    create_chain: unsafe extern "C" fn(Raw, *const Setup, *mut Raw, *mut Raw) -> i32,
    destroy_context: unsafe extern "C" fn(Raw, *mut Raw) -> u32,
    wait: unsafe extern "C" fn(Raw, Raw) -> u32,
    chain_extent: unsafe extern "C" fn(Raw, *mut Extent) -> i32,
    whole_texture: unsafe extern "C" fn(*const Resource) -> u32,
    configure: unsafe extern "C" fn(Raw, Raw, *const Configuration) -> u32,
    prepare: unsafe extern "C" fn(Raw, Raw, Raw, *const Frame, u64, u32, f32, f32) -> u32,
    generate: unsafe extern "C" fn(Raw, Raw, Raw, *mut Generated) -> u32,
    copy_present: unsafe extern "C" fn(Raw) -> u32,
    present_count: unsafe extern "C" fn(Raw, *mut u32) -> i32,
}

const _: () = {
    assert!(size_of::<Configuration>() == 48);
    assert!(size_of::<Generated>() == 104);
    assert!(size_of::<Ops>() == 18 * size_of::<Raw>());
};

#[derive(Clone, Copy, Default)]
struct Counters {
    observed: u64,
    reports: u32,
    dxgi: u32,
}

struct Fsr {
    ops: &'static Ops,
    sdk: Bindings,
    device: ComRef,
    queue: ComRef,
    log: Log,
    ui_mode: u32,
    depth_inverted: bool,
    motion_jittered: bool,
    view_space_to_meters: f32,
    state: State,
    // The presenting owner changes contexts only after the swapchain has
    // drained. A generation callback reads the currently published handle.
    generation: AtomicPtr<c_void>,
    generation_ready: Cell<bool>,
    swapchain: Cell<Raw>,
    chain: RefCell<Option<ComRef>>,
    extent: Cell<Extent>,
    sequence: Cell<u64>,
    counters: Cell<Counters>,
    capture: Capture,
    // The SDK callback returns its private configuration sequence, not a
    // source frame ID. Copy the entire source record at the handoff.
    capture_ids: Mutex<Recent<Record, CAPTURE_SLOTS>>,
    generated: AtomicU64,
    presented: AtomicU64,
    dispatched: AtomicU64,
    dispatch_result: AtomicI32,
    dispatch_logs: AtomicU32,
}

impl Fsr {
    fn chain(&self) -> Raw {
        self.chain
            .borrow()
            .as_ref()
            .map_or(null_mut(), ComRef::as_ptr)
    }

    fn next_sequence(&self) -> u64 {
        let sequence = self.sequence.get().wrapping_add(1);
        self.sequence.set(sequence);
        sequence
    }

    fn wait(&self) -> bool {
        let swapchain = self.swapchain.get();
        swapchain.is_null() || unsafe { (self.ops.wait)(self.sdk.raw, swapchain) } == 0
    }

    fn create_generation(&self, extent: Extent, directory: *const c_char) -> i32 {
        let status = self.state.read().status;
        let mut generation = null_mut();
        let result = unsafe {
            (self.ops.create_generation)(self.sdk.raw, &extent, &status, directory, &mut generation)
        };
        self.generation.store(generation, Relaxed);
        if result != OK || generation.is_null() {
            return INIT_FAILED;
        }
        self.extent.set(extent);
        self.generation_ready.set(true);
        OK
    }

    fn destroy_generation(&self) -> bool {
        self.generation_ready.set(false);
        let mut context = self.generation.load(Relaxed);
        if context.is_null() {
            return true;
        }
        let result = unsafe { (self.ops.destroy_context)(self.sdk.raw, &mut context) };
        if result == 0 {
            self.generation.store(null_mut(), Relaxed);
        } else {
            self.generation.store(context, Relaxed);
        }
        result == 0
    }

    fn apply(&self, config: &Configuration) -> bool {
        unsafe { (self.ops.configure)(self.sdk.raw, self.generation.load(Relaxed), config) == 0 }
    }

    fn recreate(&self, before: &mut Snapshot, extent: Extent) -> i32 {
        if !self.wait() {
            return NOT_READY;
        }
        if !self.state.reset_history(before.revision) {
            return NOT_READY;
        }
        before.interpolated = None;
        if !self.destroy_generation() {
            return FEATURE_FAILED;
        }
        self.create_generation(extent, std::ptr::null())
    }

    fn capture_generated(&self, generated: &Generated) {
        if !self.capture.enabled() {
            return;
        }
        let record = self
            .capture_ids
            .lock()
            .unwrap()
            .get(generated.sequence)
            .copied();
        let Some(record) = record.filter(|record| self.capture.wants(record.frame_id)) else {
            return;
        };
        if !generated.input.resource.is_null() {
            self.capture.record(
                generated.command,
                generated.input.resource,
                generated.input.state,
                &record,
                c"fg-generation-input",
                0,
                0,
                u32::MAX,
                u32::MAX,
                0.0,
            );
        }
        for (index, output) in generated
            .outputs
            .iter()
            .take(generated.count as usize)
            .enumerate()
        {
            if !output.resource.is_null() {
                self.capture.record(
                    generated.command,
                    output.resource,
                    output.state,
                    &record,
                    c"fg-generated-before-ui",
                    0,
                    index as u32,
                    generated.reset,
                    u32::MAX,
                    0.0,
                );
            }
        }
    }
}

impl Session for Fsr {
    fn state(&self) -> &State {
        &self.state
    }

    fn configure(&self, options: Options) -> i32 {
        if options.mode > RSF_FG_FIXED
            || options.reflex_mode != RSF_REFLEX_OFF
            || options.frame_limit_us != 0
        {
            return UNSUPPORTED;
        }
        let chain = self.chain();
        if chain.is_null() {
            return NOT_READY;
        }
        if options.mode != RSF_FG_OFF && options.generated_frames == 0 {
            return INVALID;
        }
        // Configure is called at every application Present.
        if self.state.read().request.is_some_and(|before| {
            before.mode == options.mode && before.generated_frames == options.generated_frames
        }) {
            return OK;
        }
        if self.generation_ready.get()
            && !self.apply(&Configuration {
                chain,
                user: std::ptr::from_ref(self).cast_mut().cast(),
                hudless: null_mut(),
                sequence: self.next_sequence(),
                width: 0,
                height: 0,
                enabled: 0,
                copy_present: 0,
            })
        {
            return FEATURE_FAILED;
        }
        self.state
            .configured(options, RSF_FG_OFF, 0, RSF_REFLEX_OFF, true);
        OK
    }

    unsafe fn prepare(&self, command: Raw, frame: *const Frame) -> i32 {
        let (frame, record) =
            match unsafe { read_frame(self.ops.check, frame, self.device.as_ptr()) } {
                Ok(frame) => frame,
                Err(result) => return result,
            };
        let chain = self.chain();
        let mut before = self.state.read();
        if chain.is_null() || record.frame_id <= before.last_prepare {
            return NOT_READY;
        }
        let enabled = frame.interpolate != 0 && before.mode() != RSF_FG_OFF;
        let hudless = enabled && self.ui_mode == HUDLESS;
        if enabled {
            let result = unsafe { (self.ops.check.command)(command, self.device.as_ptr()) };
            if result != OK {
                return result;
            }
            let mut current = Extent::default();
            if unsafe { (self.ops.chain_extent)(chain, &mut current) } < 0 {
                return FEATURE_FAILED;
            }
            if !self.generation_ready.get() || current != self.extent.get() {
                let result = self.recreate(&mut before, current);
                if result != OK {
                    return result;
                }
            }
            if record.output_width != current.width
                || record.output_height != current.height
                || record.render_width > current.width
                || record.render_height > current.height
            {
                return STALE;
            }
            if (record.camera.depth_inverted != 0) != self.depth_inverted
                || (record.camera.motion_jittered != 0) != self.motion_jittered
                || (hudless && frame.hudless.resource.is_null())
            {
                return INVALID;
            }
            for resource in [&frame.depth, &frame.motion, &frame.hudless] {
                if !resource.resource.is_null()
                    && (resource.state != NON_PIXEL_READ
                        || unsafe { (self.ops.whole_texture)(resource) } == 0)
                {
                    return INVALID;
                }
            }
        }

        if self.generation_ready.get() {
            let config = Configuration {
                chain,
                user: std::ptr::from_ref(self).cast_mut().cast(),
                hudless: if hudless {
                    frame.hudless.resource
                } else {
                    null_mut()
                },
                sequence: self.next_sequence(),
                width: record.output_width,
                height: record.output_height,
                enabled: u32::from(enabled),
                copy_present: u32::from(self.ui_mode == UI_NONE),
            };
            if self.capture.enabled() {
                self.capture_ids
                    .lock()
                    .unwrap()
                    .insert(config.sequence, *record);
            }
            if !self.apply(&config) {
                return FEATURE_FAILED;
            }
        }
        let (mode, count) = if enabled {
            (before.mode(), 1)
        } else {
            (RSF_FG_OFF, 0)
        };
        let staged = self
            .state
            .stage(before.revision, record.frame_id, mode, count);
        if !enabled {
            return configure_disabled(&self.state, before, record.frame_id, staged);
        }
        if !staged {
            return NOT_READY;
        }
        let reset = before.reset_for(record);
        // Perspective W follows view Z. A right-handed projection looks down
        // negative Z; a positive-Z view basis remains positive.
        let forward_sign = if record.camera.view_to_clip[11] < 0.0 {
            -1.0
        } else {
            1.0
        };
        let sequence = self.sequence.get();
        let dispatched = unsafe {
            (self.ops.prepare)(
                self.sdk.raw,
                self.generation.load(Relaxed),
                command,
                frame,
                sequence,
                u32::from(reset),
                self.view_space_to_meters,
                forward_sign,
            )
        };
        self.capture
            .event(record, c"fsr-prepare", sequence, reset, dispatched as i32);
        if dispatched != 0 {
            self.state.reset_history(before.revision);
            return FEATURE_FAILED;
        }
        if self
            .state
            .complete(before.revision, record.frame_id, true, mode, count)
        {
            OK
        } else {
            NOT_READY
        }
    }

    fn after(&self) -> i32 {
        let chain = self.chain();
        if chain.is_null() {
            return NOT_READY;
        }
        let before = self.state.read();
        self.capture
            .flush_after_queue(self.queue.as_ptr(), || self.wait());
        let copy_present = self.ui_mode == UI_NONE;
        let dispatched = self.dispatched.load(Relaxed);
        let generated = if copy_present {
            self.generated.load(Relaxed)
        } else {
            dispatched
        };
        let mut counters = self.counters.get();
        let mut observation = Observation {
            vendor_status: self.dispatch_result.load(Relaxed),
            statistics: RSF_FG_STAT_ACTIVITY
                | if copy_present {
                    RSF_FG_STAT_PRESENT_CALLBACKS
                } else {
                    0
                },
            callbacks: Some((generated, self.presented.load(Relaxed))),
            active: generated > counters.observed,
            ..Observation::default()
        };
        counters.observed = generated;
        let mut count = 0;
        let result = unsafe { (self.ops.present_count)(chain, &mut count) };
        if result >= 0 {
            let delta = if count >= counters.dxgi || counters.dxgi > 0xf000_0000 {
                count.wrapping_sub(counters.dxgi)
            } else {
                count
            };
            counters.dxgi = count;
            observation.presented = u64::from(delta);
            observation.statistics |= RSF_FG_STAT_TOTAL_PRESENTED;
        }
        self.state.observe(before.revision, observation);
        let report = counters.reports < 6;
        counters.reports = counters.reports.saturating_add(1);
        self.counters.set(counters);
        if report {
            self.log.write(format_args!(
                "FSR FG counters source={} generated={dispatched} DXGI_present_count={count} query={:08x}",
                before.last_prepare, result as u32
            ));
        }
        OK
    }

    fn retire(&self) -> bool {
        let chain = self.chain();
        if self.generation_ready.get()
            && !chain.is_null()
            && !self.apply(&Configuration {
                chain,
                user: null_mut(),
                hudless: null_mut(),
                sequence: self.next_sequence(),
                width: 0,
                height: 0,
                enabled: 0,
                copy_present: 0,
            })
        {
            return false;
        }
        if !self.wait() {
            return false;
        }
        // A callback may have recorded readbacks during that wait. Snapshot
        // only afterward and retire them on the game queue before dropping.
        if !self.capture.retire_after_wait(self.queue.as_ptr()) || !self.destroy_generation() {
            return false;
        }
        let chain = self.chain.borrow_mut().take();
        drop(chain);
        let mut context = self.swapchain.get();
        if !context.is_null()
            && unsafe { (self.ops.destroy_context)(self.sdk.raw, &mut context) } != 0
        {
            self.swapchain.set(context);
            return false;
        }
        self.swapchain.set(null_mut());
        true
    }
}

static PROVIDER: Provider = provider::<Fsr>();

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr12_provider() -> *const Provider {
    &PROVIDER
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr12_create(
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
    if setup.streamline_host.is_null() && unsafe { (ops.check.streamline_loaded)() } != 0 {
        return NEEDS_RESTART;
    }
    if !matches!(setup.feature_major, 3 | 4) || !matches!(setup.chain.ui_mode, UI_NONE | HUDLESS) {
        return UNSUPPORTED;
    }
    let mut bindings = null_mut();
    let result = unsafe { (ops.open)(setup, &mut bindings) };
    if result != OK {
        return result;
    }
    let sdk = Bindings {
        raw: bindings,
        close: ops.close,
    };
    let mut initial = initial_status(1);
    initial.version_id = unsafe {
        (ops.select_version)(
            sdk.raw,
            setup,
            initial.version_name.as_mut_ptr(),
            initial.version_name.len() as u32,
        )
    };
    if initial.version_id == 0 {
        return UNSUPPORTED;
    }
    let session = crate::allocate_handle(Fsr {
        ops,
        sdk,
        log: Log::new(setup),
        device: unsafe { ComRef::retain(setup.chain.d3d12_device) },
        queue: unsafe { ComRef::retain(setup.chain.d3d12_queue) },
        ui_mode: setup.chain.ui_mode,
        depth_inverted: setup.depth_inverted != 0,
        motion_jittered: setup.motion_jittered != 0,
        view_space_to_meters: setup.view_space_to_meters,
        state: State::new(initial),
        generation: AtomicPtr::new(null_mut()),
        generation_ready: Cell::new(false),
        swapchain: Cell::new(null_mut()),
        chain: RefCell::new(None),
        extent: Cell::new(Extent::default()),
        sequence: Cell::new(0),
        counters: Cell::new(Counters::default()),
        capture: Capture::from_environment(ops.capture),
        capture_ids: Mutex::new(Recent::new()),
        generated: AtomicU64::new(0),
        presented: AtomicU64::new(0),
        dispatched: AtomicU64::new(0),
        dispatch_result: AtomicI32::new(0),
        dispatch_logs: AtomicU32::new(0),
    });
    let Some(owner) = (unsafe { session.as_ref() }) else {
        return INIT_FAILED;
    };
    let extent = Extent {
        width: setup.chain.width,
        height: setup.chain.height,
        format: unsafe { (ops.surface_format)(setup.chain.format) },
    };
    let mut result = owner.create_generation(extent, setup.runtime_directory_utf8);
    if result == OK {
        result = unsafe {
            (ops.provider_matches)(
                owner.sdk.raw,
                owner.generation.load(Relaxed),
                initial.version_id,
            )
        };
    }
    if result == OK && !setup.chain.hwnd.is_null() {
        let (mut swapchain, mut created) = (null_mut(), null_mut());
        result = unsafe { (ops.create_chain)(owner.sdk.raw, setup, &mut swapchain, &mut created) };
        owner.swapchain.set(swapchain);
        *owner.chain.borrow_mut() = unsafe { ComRef::from_owned(created) };
    }
    if result != OK {
        unsafe { destroy::<Fsr>(session.cast()) };
        return result;
    }
    unsafe {
        out.write(session.cast());
        chain.write(owner.chain());
    }
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr12_generate(user: Raw, desc: Raw) -> u32 {
    let owner = unsafe { &*user.cast::<Fsr>() };
    let context = owner.generation.load(Relaxed);
    if context.is_null() {
        return VENDOR_PARAMETER_ERROR;
    }
    let mut generated = Generated::default();
    let result = unsafe { (owner.ops.generate)(owner.sdk.raw, context, desc, &mut generated) };
    if result == 0 {
        owner.capture_generated(&generated);
    }
    owner.dispatch_result.store(result as i32, Relaxed);
    if result == 0 {
        owner
            .dispatched
            .fetch_add(u64::from(generated.count), Relaxed);
    }
    let report = owner.dispatch_logs.fetch_add(1, Relaxed);
    if report < 3 || (result != 0 && report < 12) {
        owner.log.write(format_args!(
            "FSR FG dispatch frame={} requested_outputs={} result={} reset={}",
            generated.sequence, generated.count, result as i32, generated.reset
        ));
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fsr12_present(user: Raw, desc: Raw, generated: u32) -> u32 {
    let owner = unsafe { &*user.cast::<Fsr>() };
    let result = unsafe { (owner.ops.copy_present)(desc) };
    if result == 0 {
        owner.presented.fetch_add(1, Relaxed);
        if generated != 0 {
            owner.generated.fetch_add(1, Relaxed);
        }
    }
    result
}
