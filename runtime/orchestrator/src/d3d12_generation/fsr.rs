// SPDX-License-Identifier: MIT
use super::*;
use crate::com_ref::ComRef;
use crate::ffx_version::{self, MAX_VERSIONS, Version};
use crate::frame_capture12::{Owner as Capture, Platform as CapturePlatform};
use crate::render_abi::{
    RSF_FG_FIXED, RSF_FG_STAT_ACTIVITY, RSF_FG_STAT_PRESENT_CALLBACKS, RSF_FG_STAT_TOTAL_PRESENTED,
    RSF_REFLEX_OFF, rsf_backend_resource as Resource,
};
use crate::sdk_module::FFX_CONTEXT_EXPORTS;
use std::cell::{Cell, RefCell};
use std::ffi::CStr;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering::Relaxed};

const CAPTURE_SLOTS: usize = 64;
const NON_PIXEL_READ: u32 = 0x40; // D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE
const VENDOR_PARAMETER_ERROR: u32 = 6;
const MISSING_ENTRY_POINT: i32 = -5;
const MODULE: &str = "amd_fidelityfx_framegeneration_dx12.dll";
// FFX_FRAMEGENERATION_ENABLE_* values; fsr_generation.cpp asserts them.
const CREATE_DISPLAY_RESOLUTION_MOTION: u32 = 1 << 1;
const CREATE_JITTER_CANCELLATION: u32 = 1 << 2;
const CREATE_DEPTH_INVERTED: u32 = 1 << 3;
const CREATE_DEPTH_INFINITE: u32 = 1 << 4;
const CREATE_HDR: u32 = 1 << 5;
type QueryVersions =
    unsafe extern "C" fn(Raw, &Setup, &mut [Version; MAX_VERSIONS], &mut u64) -> i32;

/// SDK API versions the native leaf compiled against.
#[repr(C)]
struct SdkApi {
    generation: [u32; 3],
    swapchain: [u32; 3],
}

#[repr(C)]
struct GenerationPlan {
    version_id: u64,
    width: u32,
    height: u32,
    max_render_width: u32,
    max_render_height: u32,
    format: u32,
    flags: u32,
}

#[repr(C)]
struct PreparePlan {
    command: Raw,
    depth: Raw,
    motion: Raw,
    sequence: u64,
    render_width: u32,
    render_height: u32,
    jitter: [f32; 2],
    motion_scale: [f32; 2],
    frame_time_ms: f32,
    near_plane: f32,
    far_plane: f32,
    vertical_fov: f32,
    view_space_to_meters: f32,
    reset: u32,
    position: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
}

#[repr(C)]
struct Configuration {
    chain: Raw,
    user: Raw,
    hudless: Raw,
    sequence: u64,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    enabled: u32,
    generation_callback: u32,
    present_callback: u32,
}

/// An SDK resource with its raw FFX_API_RESOURCE_STATE_* bits.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SdkResource {
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
    input: SdkResource,
    outputs: [SdkResource; 4],
}

#[repr(C)]
#[derive(Default)]
struct PresentFacts {
    command: Raw,
    source: SdkResource,
    output: SdkResource,
}

type Entry = Option<unsafe extern "system" fn() -> isize>;

#[repr(C)]
struct Ops {
    check: &'static Checks,
    capture: &'static CapturePlatform,
    sdk: &'static SdkApi,
    resolve: Resolve,
    close: unsafe extern "C" fn(Raw),
    bind: unsafe extern "C" fn(Raw, *const Entry),
    versions: QueryVersions,
    surface_format: unsafe extern "C" fn(u32) -> u32,
    create_generation: unsafe extern "C" fn(Raw, *const GenerationPlan, *mut Raw) -> u32,
    provider_version: unsafe extern "C" fn(Raw, Raw, *mut u64) -> u32,
    create_chain: unsafe extern "C" fn(Raw, *const Setup, *mut Raw, *mut Raw, *mut u32) -> i32,
    destroy_context: unsafe extern "C" fn(Raw, *mut Raw) -> u32,
    wait: unsafe extern "C" fn(Raw, Raw) -> u32,
    chain_extent: unsafe extern "C" fn(Raw, *mut Extent) -> i32,
    whole_texture: unsafe extern "C" fn(*const Resource) -> u32,
    configure: unsafe extern "C" fn(Raw, Raw, *const Configuration) -> u32,
    prepare: unsafe extern "C" fn(Raw, Raw, *const PreparePlan) -> u32,
    generate: unsafe extern "C" fn(Raw, Raw, Raw, *mut Generated) -> u32,
    present_facts: unsafe extern "C" fn(Raw, *mut PresentFacts),
    copy: unsafe extern "C" fn(Raw, Raw, u32, Raw, u32),
    present_count: unsafe extern "C" fn(Raw, *mut u32) -> i32,
}

const _: () = {
    assert!(size_of::<SdkApi>() == 24 && size_of::<GenerationPlan>() == 32);
    assert!(size_of::<PreparePlan>() == 128);
    assert!(size_of::<Configuration>() == 64);
    assert!(size_of::<Generated>() == 104 && size_of::<PresentFacts>() == 40);
    assert!(size_of::<Entry>() == size_of::<Raw>());
    assert!(size_of::<Ops>() == 21 * size_of::<Raw>());
};

// rsf_fsr12_entry_index order. The shared names are create, destroy, query, dispatch.
const ENTRY_NAMES: [&CStr; 5] = [
    FFX_CONTEXT_EXPORTS[0],
    FFX_CONTEXT_EXPORTS[1],
    FFX_CONTEXT_EXPORTS[2],
    FFX_CONTEXT_EXPORTS[3],
    c"ffxConfigure",
];
const QUERY_ENTRY: usize = 2;

/// All SDK exports the owner calls, or None when any is missing.
fn resolve_entries(module: &SdkModule) -> Option<[Entry; ENTRY_NAMES.len()]> {
    let entries = ENTRY_NAMES.map(|name| module.symbol(name));
    entries.iter().all(Option::is_some).then_some(entries)
}

// FFX_API_RESOURCE_STATE_* bits and the D3D12 state each adds; fsr_generation.cpp
// asserts both sides. COMMON and PRESENT are D3D12 state 0.
const STATE_COMMON: u32 = 1;
const STATE_PRESENT: u32 = 1 << 7;
const STATE_MAP: [(u32, u32); 8] = [
    (1 << 1, 0x8),   // UNORDERED_ACCESS
    (1 << 2, 0x40),  // COMPUTE_READ: NON_PIXEL_SHADER_RESOURCE
    (1 << 3, 0x80),  // PIXEL_READ: PIXEL_SHADER_RESOURCE
    (1 << 4, 0x800), // COPY_SRC: COPY_SOURCE
    (1 << 5, 0x400), // COPY_DEST
    (1 << 6, 0x200), // INDIRECT_ARGUMENT
    (1 << 8, 0x4),   // RENDER_TARGET
    (1 << 9, 0x10),  // DEPTH_ATTACHMENT: DEPTH_WRITE
];

/// D3D12 state for an FFX resource state, or None for bits this owner does not know.
fn d3d12_state(ffx: u32) -> Option<u32> {
    let known = STATE_MAP
        .iter()
        .fold(STATE_COMMON | STATE_PRESENT, |known, &(bit, _)| known | bit);
    (ffx & !known == 0).then(|| {
        STATE_MAP
            .iter()
            .filter(|&&(bit, _)| ffx & bit != 0)
            .fold(0, |state, &(_, d3d12)| state | d3d12)
    })
}

impl SdkResource {
    /// The resource with its D3D12 state, when both are usable.
    fn native(self) -> Option<(Raw, u32)> {
        let state = d3d12_state(self.state)?;
        (!self.resource.is_null()).then_some((self.resource, state))
    }
}
/// FFX frame-generation create flags for the setup's declared inputs.
fn create_flags(setup: &Setup) -> u32 {
    [
        (setup.depth_inverted, CREATE_DEPTH_INVERTED),
        (setup.depth_infinite, CREATE_DEPTH_INFINITE),
        (setup.motion_jittered, CREATE_JITTER_CANCELLATION),
        (
            setup.motion_at_display_resolution,
            CREATE_DISPLAY_RESOLUTION_MOTION,
        ),
        (setup.hdr, CREATE_HDR),
    ]
    .into_iter()
    .filter(|&(enabled, _)| enabled != 0)
    .fold(0, |flags, (_, flag)| flags | flag)
}

/// Prepare constants for one source frame. Position, right and up are the
/// view-to-world rows; forward carries the caller's handedness sign.
fn prepare_plan(
    command: Raw,
    frame: &Frame,
    record: &Record,
    sequence: u64,
    reset: bool,
    meters: f32,
    forward_sign: f32,
) -> PreparePlan {
    let camera = &record.camera;
    let world = &camera.view_to_world;
    let row = |start: usize| [world[start], world[start + 1], world[start + 2]];
    PreparePlan {
        command,
        depth: frame.depth.resource,
        motion: frame.motion.resource,
        sequence,
        render_width: record.render_width,
        render_height: record.render_height,
        jitter: camera.jitter_pixels,
        motion_scale: [frame.motion_scale_x, frame.motion_scale_y],
        frame_time_ms: record.frame_time_ms,
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        vertical_fov: camera.vertical_fov_radians,
        view_space_to_meters: meters,
        reset: u32::from(reset),
        position: row(12),
        right: row(0),
        up: row(4),
        forward: row(8).map(|value| forward_sign * value),
    }
}

/// Borrowed NUL-terminated text for a log record.
unsafe fn c_text(text: *const c_char) -> String {
    if text.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(text) }
        .to_string_lossy()
        .into_owned()
}

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
    create_flags: u32,
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

    /// Creates the generation context for `extent`. `startup` is present only
    /// for the first context and selects its startup record.
    fn create_generation(&self, extent: Extent, startup: Option<&Setup>) -> i32 {
        let status = self.state.read().status;
        let plan = GenerationPlan {
            version_id: status.version_id,
            width: extent.width,
            height: extent.height,
            // Render size never exceeds the swapchain; prepare refuses larger.
            max_render_width: extent.width,
            max_render_height: extent.height,
            format: extent.format,
            flags: self.create_flags,
        };
        let mut generation = null_mut();
        let result = unsafe { (self.ops.create_generation)(self.sdk.raw, &plan, &mut generation) };
        if let Some(setup) = startup.filter(|setup| !setup.runtime_directory_utf8.is_null()) {
            let [major, minor, patch] = self.ops.sdk.generation;
            let (selected, root) = unsafe {
                (
                    c_text(status.version_name.as_ptr()),
                    c_text(setup.runtime_directory_utf8),
                )
            };
            self.log.write(format_args!(
                "FSR FG create generation: FFX result={result} requested_major={} selected={selected} id={} API={major}.{minor}.{patch} root={root}",
                setup.feature_major, status.version_id
            ));
        }
        self.generation.store(generation, Relaxed);
        if result != 0 || generation.is_null() {
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

    /// Configuration for the next SDK sequence. An attached owner receives the
    /// generation callback, and the present callback when it composes no UI
    /// and copies each presented image itself. Generation covers the whole
    /// `width` by `height` back buffer.
    fn configuration(
        &self,
        chain: Raw,
        attached: bool,
        hudless: Raw,
        (width, height): (u32, u32),
        enabled: bool,
        copy_present: bool,
    ) -> Configuration {
        Configuration {
            chain,
            user: if attached {
                std::ptr::from_ref(self).cast_mut().cast()
            } else {
                null_mut()
            },
            hudless,
            sequence: self.next_sequence(),
            left: 0,
            top: 0,
            width: width as i32,
            height: height as i32,
            enabled: u32::from(enabled),
            generation_callback: u32::from(attached),
            present_callback: u32::from(attached && copy_present),
        }
    }

    /// Present-callback copy of the presented image into the SDK's output
    /// buffer, when they differ. Both keep the state they arrived in.
    fn copy_present(&self, present: &PresentFacts) -> u32 {
        let (Some((source, source_state)), Some((output, output_state))) =
            (present.source.native(), present.output.native())
        else {
            return VENDOR_PARAMETER_ERROR;
        };
        if present.command.is_null() {
            return VENDOR_PARAMETER_ERROR;
        }
        if source != output {
            unsafe { (self.ops.copy)(present.command, output, output_state, source, source_state) };
        }
        0
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
        self.create_generation(extent, None)
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
        if let Some((resource, state)) = generated.input.native() {
            self.capture.record(
                generated.command,
                resource,
                state,
                &record,
                c"fg-generation-input",
                0,
                0,
                u32::MAX,
                u32::MAX,
                0.0,
            );
        }
        // The SDK fills `count` of its four output slots; the rest are stale.
        for (index, output) in generated
            .outputs
            .iter()
            .take(generated.count as usize)
            .enumerate()
        {
            if let Some((resource, state)) = output.native() {
                self.capture.record(
                    generated.command,
                    resource,
                    state,
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
        // A mode change disables generation until the next prepare and keeps
        // only the generation callback installed.
        if self.generation_ready.get()
            && !self.apply(&self.configuration(chain, true, null_mut(), (0, 0), false, false))
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
            let config = self.configuration(
                chain,
                true,
                if hudless {
                    frame.hudless.resource
                } else {
                    null_mut()
                },
                (record.output_width, record.output_height),
                enabled,
                self.ui_mode == UI_NONE,
            );
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
        let plan = prepare_plan(
            command,
            frame,
            record,
            sequence,
            reset,
            self.view_space_to_meters,
            forward_sign,
        );
        let dispatched =
            unsafe { (self.ops.prepare)(self.sdk.raw, self.generation.load(Relaxed), &plan) };
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
            let delta = crate::fsr4_fg_bridge::present_count_delta(counters.dxgi, count);
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
            && !self.apply(&self.configuration(chain, false, null_mut(), (0, 0), false, false))
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

fn select_version(handle: Raw, setup: &Setup, query: QueryVersions) -> Option<Version> {
    let mut versions = [Version::default(); MAX_VERSIONS];
    let mut count = 0;
    if unsafe { query(handle, setup, &mut versions, &mut count) } != OK {
        return None;
    }
    let versions = versions.get(..usize::try_from(count).ok()?)?;
    ffx_version::select(versions, setup.feature_major, setup.version_id)
        .filter(|version| version.id != 0)
        .copied()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fsr12_probe(checks: &Checks, setup: &Setup, query: QueryVersions) -> u32 {
    let Some(module) = checks.module(setup, MODULE) else {
        return 0;
    };
    // The native query receives the resolved ffxQuery export.
    let Some(entry) = module.symbol(ENTRY_NAMES[QUERY_ENTRY]) else {
        return 0;
    };
    u32::from(select_version(entry as Raw, setup, query).is_some())
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
    let sdk = match Bindings::open(setup, ops.check, MODULE, None, ops.resolve, ops.close) {
        Ok(sdk) => sdk,
        Err(result) => return result,
    };
    let Some(entries) = resolve_entries(&sdk.module) else {
        return MISSING_ENTRY_POINT;
    };
    unsafe { (ops.bind)(sdk.raw, entries.as_ptr()) };
    let mut initial = initial_status(1);
    let Some(version) = select_version(sdk.raw, setup, ops.versions) else {
        return UNSUPPORTED;
    };
    initial.version_id = version.id;
    if !version.name.is_null() {
        let name = unsafe { CStr::from_ptr(version.name) }.to_bytes();
        let capacity = initial.version_name.len() - 1;
        for (out, &byte) in initial.version_name[..capacity].iter_mut().zip(name) {
            *out = byte as c_char;
        }
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
        create_flags: create_flags(setup),
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
    let mut result = owner.create_generation(extent, Some(setup));
    if result == OK {
        // The SDK may fall back to another provider; only the selected one is admitted.
        let mut actual = 0;
        let queried = unsafe {
            (ops.provider_version)(owner.sdk.raw, owner.generation.load(Relaxed), &mut actual)
        };
        if queried != 0 || actual != initial.version_id {
            result = UNSUPPORTED;
        }
    }
    if result == OK && !setup.chain.hwnd.is_null() {
        let (mut swapchain, mut created, mut ffx) = (null_mut(), null_mut(), u32::MAX);
        result = unsafe {
            (ops.create_chain)(owner.sdk.raw, setup, &mut swapchain, &mut created, &mut ffx)
        };
        owner.swapchain.set(swapchain);
        if result == OK {
            let [major, minor, patch] = ops.sdk.swapchain;
            let desc = &setup.chain;
            owner.log.write(format_args!(
                "FSR FG create swapchain: FFX result={ffx} chain={} API={major}.{minor}.{patch} size={}x{} buffers={} format={}",
                u32::from(!created.is_null()),
                desc.width,
                desc.height,
                desc.buffer_count,
                desc.format
            ));
            // Adopt the SDK's swapchain reference only from a successful create.
            if ffx == 0 && !created.is_null() {
                *owner.chain.borrow_mut() = unsafe { ComRef::from_owned(created) };
            } else {
                result = INIT_FAILED;
            }
        }
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
    let mut present = PresentFacts::default();
    unsafe { (owner.ops.present_facts)(desc, &mut present) };
    let result = owner.copy_present(&present);
    if result == 0 {
        owner.presented.fetch_add(1, Relaxed);
        if generated != 0 {
            owner.generated.fetch_add(1, Relaxed);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_flags_follow_declared_inputs() {
        let mut setup: Setup = unsafe { std::mem::zeroed() };
        assert_eq!(create_flags(&setup), 0);
        setup.depth_inverted = 1;
        setup.motion_jittered = 1;
        setup.hdr = 1;
        assert_eq!(
            create_flags(&setup),
            CREATE_DEPTH_INVERTED | CREATE_JITTER_CANCELLATION | CREATE_HDR
        );
        setup.depth_infinite = 1;
        setup.motion_at_display_resolution = 1;
        assert_eq!(create_flags(&setup), 0b11_1110);
    }

    #[test]
    fn sdk_states_map_to_d3d12_and_refuse_unknown_bits() {
        assert_eq!(d3d12_state(STATE_COMMON), Some(0));
        assert_eq!(d3d12_state(STATE_PRESENT), Some(0));
        // PIXEL_COMPUTE_READ reads in both shader stages.
        assert_eq!(d3d12_state(0b1100), Some(0xc0));
        assert_eq!(d3d12_state((1 << 4) | (1 << 8)), Some(0x804));
        assert_eq!(d3d12_state(1 << 10), None);
        let resource = SdkResource {
            resource: std::ptr::dangling_mut(),
            state: 1 << 5,
        };
        assert_eq!(resource.native(), Some((resource.resource, 0x400)));
        let missing = SdkResource {
            state: 1 << 5,
            ..SdkResource::default()
        };
        assert_eq!(missing.native(), None);
    }

    #[test]
    fn prepare_plan_reads_view_basis_and_signs_forward() {
        let frame: Frame = unsafe { std::mem::zeroed() };
        let mut record: Record = unsafe { std::mem::zeroed() };
        record.camera.view_to_world = std::array::from_fn(|index| index as f32);
        record.render_width = 1280;
        record.camera.jitter_pixels = [0.25, -0.5];
        let plan = prepare_plan(null_mut(), &frame, &record, 9, true, 0.01, -1.0);
        assert_eq!(plan.right, [0.0, 1.0, 2.0]);
        assert_eq!(plan.up, [4.0, 5.0, 6.0]);
        assert_eq!(plan.forward, [-8.0, -9.0, -10.0]);
        assert_eq!(plan.position, [12.0, 13.0, 14.0]);
        assert_eq!((plan.sequence, plan.reset, plan.render_width), (9, 1, 1280));
        assert_eq!(plan.jitter, [0.25, -0.5]);
        assert_eq!(plan.view_space_to_meters, 0.01);
    }
}
