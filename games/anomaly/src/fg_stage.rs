// SPDX-License-Identifier: MIT
//! Joins the native engine boundaries to the Runtime's D3D11 FG and presentation tables.

mod camera;
mod d3d;
#[cfg(all(feature = "fg-fixture", not(test)))]
mod fixture;
mod options;
#[cfg(test)]
mod tests;
mod window;

use self::camera::Camera;
pub(crate) use self::options::CaptureSettings;
use self::options::{Options, backend_name};
use crate::fg_inputs::Frame as InputFrame;
use crate::fg_inputs::{Owner as InputPass, Result as InputResult, StateScope};
use crate::fg_policy::{Counters, Decision, Policy, RSF_ANOMALY_RESET_NONE, RSF_ANOMALY_SKIP_NONE};
use crate::host_log::Log;
use crate::sys::rsf_anomaly_motion_frame_target;
use crate::sys::*;
use crate::sys::{
    RSF_ANOMALY_SR_ENGINE_VERSION, RSF_ANOMALY_SR_PROJECT_ID, RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS,
};
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Mutex, OnceLock};
use std::{mem, ptr};
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

type Raw = *mut c_void;
type Pass = rsf_game_render_pass;
type Status = rsf_anomaly_fg_status;
type Platform = rsf_anomaly_fg_platform;
#[derive(Clone, Copy)]
pub(crate) struct Frontend {
    pub enabled: u32,
    pub backend: u32,
    pub generated: u32,
    pub latency: u32,
    pub frame_limit_us: u32,
}

const MAIN_VIEW: u64 = 1;
const VIEWPORT: u64 = 1;
const RHI_VIEWPORT: u64 = 1;
const DXGI_PRESENT_TEST: u32 = 1;

#[derive(Clone, Copy)]
struct PresentBindings {
    install: unsafe extern "C" fn(setup: *const rsf_d3d11_present_setup) -> i32,
    is_owner: unsafe extern "C" fn(swapchain: *mut c_void) -> i32,
    has_owner: unsafe extern "C" fn() -> i32,
    backend: unsafe extern "C" fn() -> u32,
    backend_choices: unsafe extern "C" fn() -> u32,
    generation: unsafe extern "C" fn() -> u64,
    switch_result: unsafe extern "C" fn() -> i32,
}

#[derive(Clone, Copy)]
struct FgBindings {
    set_log: unsafe extern "C" fn(log: rsf_backend_log_fn, user: *mut c_void),
    options: unsafe extern "C" fn(options: *const rsf_native_fg_options),
    status: unsafe extern "C" fn(status: *mut rsf_native_fg_status) -> i32,
    cpu: unsafe extern "C" fn(event: *const rsf_game_cpu_event) -> i32,
    pass: unsafe extern "C" fn(
        context11: *mut c_void,
        pass: *const rsf_game_render_pass,
        begin: u32,
    ) -> i32,
    bind_source: unsafe extern "C" fn(identity: *const rsf_game_render_pass),
    capture: unsafe extern "C" fn(
        context11: *mut c_void,
        identity: *const rsf_game_render_pass,
        inputs: *const rsf_d3d11_fg_inputs,
    ),
    inputs_ready: unsafe extern "C" fn(source_frame_id: u64) -> i32,
    dump_inputs: unsafe extern "C" fn(
        context11: *mut c_void,
        source_frame_id: u64,
        directory_utf8: *const c_char,
        prefix_utf8: *const c_char,
    ) -> i32,
    prepare: unsafe extern "C" fn(
        user: *mut c_void,
        chain11: *mut c_void,
        context11: *mut c_void,
        list12: *mut c_void,
        backbuffer12: *mut c_void,
        host: *mut rsf_streamline_host,
        provider: *mut c_void,
        sync: u32,
    ),
    retire: unsafe extern "C" fn(user: *mut c_void, provider: *mut c_void),
    present_event:
        unsafe extern "C" fn(user: *mut c_void, event: *const rsf_observer_present_event),
    latency_event:
        unsafe extern "C" fn(user: *mut c_void, event: *const rsf_observer_present_event),
}

struct EngineBindings {
    state_scope: StateScope,
    engine_depth_view: unsafe extern "C" fn(rva: u32) -> *mut c_void,
    sr_owns_depth: fn(native_frame: u32) -> bool,
    motion_target: fn() -> rsf_anomaly_motion_frame_target,
    graphics: d3d::Graphics,
}

impl EngineBindings {
    unsafe fn bind(platform: *const Platform) -> Option<Self> {
        let platform = unsafe { full_table(platform) }?;
        Some(Self {
            state_scope: StateScope::bind(platform.state_scope)?,
            engine_depth_view: platform.engine_depth_view?,
            sr_owns_depth: crate::sr_stage::owns_depth,
            motion_target: crate::motion::frame_target,
            graphics: d3d::Graphics::default(),
        })
    }
}

// All callers supply a readable size prefix. No full reference is formed for a short table.
unsafe fn full_table<'a, T>(table: *const T) -> Option<&'a T> {
    if table.is_null() || unsafe { table.cast::<u32>().read() } < size_of::<T>() as u32 {
        return None;
    }
    Some(unsafe { &*table })
}

struct Tables {
    present: PresentBindings,
    fg: FgBindings,
}

impl Tables {
    fn bind(present: rsf_d3d11_present_api, fg: rsf_d3d11_fg_api) -> Option<Self> {
        if present.struct_size < size_of::<rsf_d3d11_present_api>() as u32
            || present.abi_version != RSF_D3D11_PRESENT_API_VERSION
            || fg.struct_size < size_of::<rsf_d3d11_fg_api>() as u32
            || fg.abi_version != RSF_D3D11_FG_API_VERSION
        {
            return None;
        }
        fg.reflex_ordering?;
        Some(Self {
            present: PresentBindings {
                install: present.install?,
                is_owner: present.is_owner?,
                has_owner: present.has_owner?,
                backend: present.backend?,
                backend_choices: present.backend_choices?,
                generation: present.generation?,
                switch_result: present.switch_result?,
            },
            fg: FgBindings {
                set_log: fg.set_log?,
                options: fg.options?,
                status: fg.status?,
                cpu: fg.cpu?,
                pass: fg.pass?,
                bind_source: fg.bind_source?,
                capture: fg.capture?,
                inputs_ready: fg.inputs_ready?,
                dump_inputs: fg.dump_inputs?,
                prepare: fg.prepare?,
                retire: fg.retire?,
                present_event: fg.present_event?,
                latency_event: fg.latency_event?,
            },
        })
    }

    fn query(runtime: HMODULE, log: Log) -> Option<Self> {
        use crate::runtime_module::export;
        let present_query = unsafe {
            export::<rsf_get_d3d11_present_api_fn>(runtime, RSF_D3D11_PRESENT_API_ENTRY_POINT)
        }
        .flatten();
        let fg_query =
            unsafe { export::<rsf_get_d3d11_fg_api_fn>(runtime, RSF_D3D11_FG_API_ENTRY_POINT) }
                .flatten();
        let (Some(present_query), Some(fg_query)) = (present_query, fg_query) else {
            let required = if present_query.is_none() {
                "rsf_get_d3d11_present_api"
            } else {
                "rsf_get_d3d11_fg_api"
            };
            log.say(format!(
                "Anomaly FG refused: the Runtime has no {required}."
            ));
            return None;
        };
        let mut present = rsf_d3d11_present_api {
            struct_size: size_of::<rsf_d3d11_present_api>() as u32,
            abi_version: RSF_D3D11_PRESENT_API_VERSION,
            ..Default::default()
        };
        let mut fg = rsf_d3d11_fg_api {
            struct_size: size_of::<rsf_d3d11_fg_api>() as u32,
            abi_version: RSF_D3D11_FG_API_VERSION,
            ..Default::default()
        };
        if unsafe { present_query(&mut present) == 0 && fg_query(&mut fg) == 0 }
            && let Some(tables) = Self::bind(present, fg)
        {
            return Some(tables);
        }
        log.text(c"Anomaly FG refused: Runtime FG/presentation tables refused (version or size).");
        None
    }
}

struct Cold {
    configured: Option<(Options, Log)>,
    attempted: bool,
}

static COLD: Mutex<Cold> = Mutex::new(Cold {
    configured: None,
    attempted: false,
});
static STAGE: OnceLock<Stage> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);
static OVERLAY: OnceLock<(rsf_anomaly_fg_overlay_present, usize)> = OnceLock::new();

impl Cold {
    fn configure(options: Options, log: Log) {
        let mut cold = COLD.lock().unwrap();
        if !cold.attempted {
            cold.configured = Some((options, log));
        }
    }

    fn attempt() -> Option<(Options, Log)> {
        let mut cold = COLD.lock().unwrap();
        if cold.attempted || !cold.configured.as_ref().is_some_and(|(o, _)| o.bridge) {
            return None;
        }
        cold.attempted = true;
        cold.configured.take()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Open,
    Rendering,
}

#[derive(Clone, Copy, Default)]
struct Render {
    native_frame: u32,
    extent: [u32; 2],
    camera: Option<Camera>,
    frame_time: f32,
    invalidated: bool,
}

impl Render {
    fn motion_input(&self, target: rsf_anomaly_motion_frame_target) -> Raw {
        if target.written
            && target.main
            && target.frame == self.native_frame
            && [target.width, target.height] == self.extent
        {
            target.texture
        } else {
            ptr::null_mut()
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Totals {
    policy: Counters,
    runtime_tagged: u64,
    vendor_presented: Option<PresentTotal>,
    captures: u64,
}

#[derive(Clone, Copy)]
struct PresentTotal {
    generation: u64,
    count: u64,
}

impl PresentTotal {
    fn delta(self, previous: Self) -> Option<u64> {
        if self.generation != previous.generation {
            return None;
        }
        self.count.checked_sub(previous.count)
    }
}

struct Engine {
    policy: Policy,
    phase: Phase,
    source: u64,
    input_native_frame: Option<u32>,
    render: Render,
    window: Option<Pass>,
    camera_now: Option<Camera>,
    camera_fed: Option<Camera>,
    previous_begin: i64,
    inputs: Option<InputPass>,
    input_result: InputResult,
    reported: Totals,
}

impl Engine {
    fn new(scope_hold: u32) -> Self {
        Self {
            policy: Policy::new(scope_hold),
            phase: Phase::Idle,
            source: 0,
            input_native_frame: None,
            render: Render::default(),
            window: None,
            camera_now: None,
            camera_fed: None,
            previous_begin: 0,
            inputs: None,
            input_result: InputResult::Ok,
            reported: Totals::default(),
        }
    }

    fn reset_device(&mut self) -> (Option<Pass>, Option<InputPass>) {
        self.policy.device_reset();
        (self.window.take(), self.discard_inputs())
    }

    fn retire_inputs(&mut self) -> Result<Option<InputPass>, ()> {
        if self.phase == Phase::Rendering || self.window.is_some() {
            return Err(());
        }
        self.phase = Phase::Idle;
        self.input_native_frame = None;
        self.policy.invalidate_history();
        Ok(self.discard_inputs())
    }

    fn discard_inputs(&mut self) -> Option<InputPass> {
        self.camera_now = None;
        self.camera_fed = None;
        self.previous_begin = 0;
        self.render.invalidated = true;
        self.inputs.take()
    }
}

// SAFETY: the engine serializes render and reset callbacks on the D3D11 owner. The mutex also
// protects the source facts read by a window message. It is never held across a Runtime or COM call.
unsafe impl Send for Engine {}

struct Stage {
    options: Options,
    log: Log,
    tables: Tables,
    platform: EngineBindings,
    depth_rva: u32,
    session: u64,
    frequency: u64,
    facade: AtomicPtr<c_void>,
    engine: Mutex<Engine>,
    status: Mutex<Status>,
}

// SAFETY: COM access and native engine reads run on the serialized render thread.
// Shared state uses locks or atomics.
unsafe impl Send for Stage {}
unsafe impl Sync for Stage {}

impl Stage {
    fn install(
        options: Options,
        log: Log,
        tables: Tables,
        platform: EngineBindings,
        depth_rva: u32,
    ) -> bool {
        let mut frequency = 0;
        unsafe { QueryPerformanceFrequency(&mut frequency) };
        let engine = Engine::new(options.scope_hold);
        // The bridge may call back during installation, so publish the bound tables first.
        if STAGE
            .set(Self {
                options,
                log,
                tables,
                platform,
                depth_rva,
                session: qpc() as u64,
                frequency: frequency as u64,
                facade: AtomicPtr::new(ptr::null_mut()),
                engine: Mutex::new(engine),
                status: Mutex::new(Status::default()),
            })
            .is_err()
        {
            return false;
        }
        STAGE.get().unwrap().start_bridge()
    }

    fn start_bridge(&self) -> bool {
        let options = &self.options;
        let frontend = &options.frontend;
        let fg = self.tables.fg;
        let requested = rsf_native_fg_options {
            struct_size: size_of::<rsf_native_fg_options>() as u32,
            mode: if frontend.enabled != 0 {
                RSF_FG_FIXED
            } else {
                RSF_FG_OFF
            },
            generated_frames: frontend.generated,
            reflex_mode: frontend.latency,
            frame_limit_us: frontend.frame_limit_us,
            debug: u32::from(options.debug),
        };
        let setup = rsf_d3d11_present_setup {
            struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
            runtime_directory_utf8: options.streamline.as_ptr(),
            development_runtime: u32::from(options.development),
            log: Some(bridge_log),
            accept_window: Some(window::accept_window),
            before_present: Some(before_present),
            present_event: Some(present_event),
            latency_event: Some(fg.latency_event),
            retire: Some(fg.retire),
            prepare: Some(fg.prepare),
            debug_timing: u32::from(options.debug),
            backend: frontend.backend,
            max_generated_frames: u32::MAX,
            // X-Ray uses metres and finite, standard depth.
            units_to_meters: RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS as f32,
            runtime_switching: 1,
            ui_mode: RSF_UI_MODE_BACKBUFFER_HUDLESS,
            streamline_directory_utf8: options.streamline.as_ptr(),
            fsr3_directory_utf8: options.fidelityfx.as_ptr(),
            fsr4_directory_utf8: options.fidelityfx.as_ptr(),
            xess_directory_utf8: options.xess.as_ptr(),
            engine_version_utf8: RSF_ANOMALY_SR_ENGINE_VERSION.as_ptr().cast(),
            project_id_utf8: RSF_ANOMALY_SR_PROJECT_ID.as_ptr().cast(),
            ..Default::default()
        };
        let installed = unsafe {
            (fg.set_log)(Some(bridge_log), ptr::null_mut());
            (fg.options)(&requested);
            (self.tables.present.install)(&setup) != 0
        };
        let backend = backend_name(frontend.backend);
        if !installed {
            self.log.say(format!(
                "Anomaly FG refused: the presentation bridge did not install (backend {backend}); presentation stays stock."
            ));
            return false;
        }
        INSTALLED.store(true, Ordering::Release);
        self.log.say(format!(
            "Anomaly FG: presentation bridge installed at cold start, before device creation. FG={} backend={backend} generated={} Latency={} FrameLimitUs={} scope_hold={}.",
            frontend.enabled, frontend.generated, frontend.latency, frontend.frame_limit_us, options.scope_hold
        ));
        self.publish();
        true
    }

    fn owns(&self, chain: Raw) -> bool {
        !chain.is_null() && unsafe { (self.tables.present.is_owner)(chain) != 0 }
    }

    fn live_chain(&self) -> Option<Raw> {
        let chain = self.facade.load(Ordering::Acquire);
        self.owns(chain).then_some(chain)
    }

    fn cpu(&self, source: u64, stage: u32, expected: bool, kinds: u32, message: u32, flags: u32) {
        let event = rsf_game_cpu_event {
            struct_size: size_of::<rsf_game_cpu_event>() as u32,
            stage,
            session_id: self.session,
            source_frame_id: source,
            timestamp_qpc: qpc() as u64,
            qpc_frequency: self.frequency,
            render_expected: u32::from(expected),
            input_kind: kinds,
            message_id: message,
            event_version: RSF_GAME_CPU_EVENT_VERSION,
            flags: RSF_GAME_CPU_INPUT_KEYED | flags,
        };
        unsafe { (self.tables.fg.cpu)(&event) };
    }

    fn boundary(&self, source: u64, stage: u32) {
        self.cpu(source, stage, true, 0, 0, 0);
    }

    fn open_frame(&self) -> u64 {
        let source = {
            let mut engine = self.engine.lock().unwrap();
            engine.source += 1;
            engine.input_native_frame = None;
            engine.source
        };
        // Reservation is an identity fact. Only the verified input callback emits INPUT_SAMPLE.
        self.boundary(source, RSF_GAME_CPU_FRAME_BEGIN);
        self.engine.lock().unwrap().phase = Phase::Open;
        source
    }

    fn pass(&self, context: Raw, pass: &Pass, begin: bool) -> bool {
        unsafe { (self.tables.fg.pass)(context, pass, u32::from(begin)) != 0 }
    }

    fn bind_source(&self, identity: Option<&Pass>) {
        unsafe { (self.tables.fg.bind_source)(identity.map_or(ptr::null(), ptr::from_ref)) };
    }

    fn inputs_ready(&self, source: u64) -> bool {
        unsafe { (self.tables.fg.inputs_ready)(source) != 0 }
    }

    fn capture_adapter_inputs(
        &self,
        context: Raw,
        mut identity: Pass,
        render: Render,
        previous: Option<Camera>,
    ) {
        // A reduced world frame needs the depth supplied by the SR stage.
        if (self.platform.sr_owns_depth)(render.native_frame) {
            return;
        }
        let Some(camera) = render.camera else { return };
        let reset = identity.flags & RSF_GAME_RENDER_RESET != 0;
        let previous = if reset {
            camera
        } else {
            previous.unwrap_or(camera)
        };
        let Some((camera, previous_clip_to_clip)) =
            camera.frame(previous, render.extent, render.frame_time)
        else {
            return;
        };
        identity.camera = camera;
        identity.previous_clip_to_clip = previous_clip_to_clip;
        identity.camera_valid = 1;

        let view = unsafe { (self.platform.engine_depth_view)(self.depth_rva) };
        let Some(depth) = (unsafe { d3d::depth_texture(view) }) else {
            return;
        };
        let target = (self.platform.motion_target)();
        let motion = render.motion_input(target);
        let [width, height] = render.extent;
        let frame = InputFrame {
            depth: windows::core::Interface::as_raw(&depth),
            motion,
            clip_to_previous_clip: identity.camera.clip_to_previous_clip,
            width,
            height,
        };
        let previous_inputs = self.engine.lock().unwrap().inputs.take();
        let mut pass = previous_inputs.unwrap_or_else(|| InputPass::new(self.platform.state_scope));
        let result = pass.run(context, &frame, self.log.callback, self.log.user as Raw);
        if let Some([depth, motion]) = pass.outputs() {
            let inputs = rsf_d3d11_fg_inputs {
                struct_size: size_of::<rsf_d3d11_fg_inputs>() as u32,
                abi_version: RSF_D3D11_FG_INPUTS_VERSION,
                depth,
                motion,
            };
            // Keep the pass's output leases alive until the Runtime has copied both inputs.
            unsafe { (self.tables.fg.capture)(context, &identity, &inputs) };
        }
        let mut engine = self.engine.lock().unwrap();
        engine.inputs = Some(pass);
        if result != InputResult::Ok && result != engine.input_result {
            engine.input_result = result;
            drop(engine);
            self.log.say(format!(
                "Anomaly FG: SR-off depth/motion pass refused ({})",
                result as u32
            ));
        }
    }

    fn maybe_dump(&self, context: Raw, source: u64, native_frame: u32, decision: Decision) {
        if decision.skip != RSF_ANOMALY_SKIP_NONE {
            return;
        }
        let claim = self.options.capture.lock().unwrap().claim(native_frame);
        let Some((directory, generation)) = claim else {
            return;
        };
        let prefix = CString::new(format!("fg_f{native_frame}_s{source}")).unwrap();
        let written = unsafe {
            (self.tables.fg.dump_inputs)(context, source, directory.as_ptr(), prefix.as_ptr())
        };
        self.options
            .capture
            .lock()
            .unwrap()
            .finish(native_frame, generation, written > 0);
        if written > 0 {
            self.log.say(format!(
                "Anomaly FG capture {}: {written} inputs (HUD-less, depth, motion) -> {}",
                prefix.to_string_lossy(),
                directory.to_string_lossy()
            ));
        }
    }

    fn runtime_status(&self) -> Option<rsf_native_fg_status> {
        let mut runtime = rsf_native_fg_status {
            struct_size: size_of::<rsf_native_fg_status>() as u32,
            ..Default::default()
        };
        (unsafe { (self.tables.fg.status)(&mut runtime) } != 0).then_some(runtime)
    }

    fn publish(&self) {
        let runtime = self.runtime_status();
        let counts = self.engine.lock().unwrap().policy.counters();
        let captures = self.options.capture.lock().unwrap().total;
        let present = self.tables.present;
        let (owner, backend, choices) = unsafe {
            (
                (present.has_owner)() != 0,
                (present.backend)(),
                (present.backend_choices)(),
            )
        };
        let mut status = self.status.lock().unwrap();
        status.installed = true;
        status.owner = owner;
        status.backend = backend;
        status.choices = choices;
        if let Some(runtime) = runtime {
            status.requested_mode = runtime.requested.mode;
            status.effective_mode = runtime.vendor.effective_mode;
            status.generated = runtime.vendor.effective_generated_frames;
            status.reason = runtime.reason;
            status.runtime_tagged = runtime.tagged_frames;
            status.vendor_presented = runtime.vendor.total_presented;
        }
        status.source_frames = counts.source_frames;
        status.level_frames = counts.level_frames;
        status.eligible_at_ui_boundary = counts.eligible_at_ui_boundary;
        status.fed = counts.fed;
        status.presented = counts.presented;
        status.unpresented = counts.unpresented;
        status.captures = captures;
    }
}

fn qpc() -> i64 {
    let mut value = 0;
    unsafe { QueryPerformanceCounter(&mut value) };
    value
}

fn installed() -> Option<&'static Stage> {
    if INSTALLED.load(Ordering::Acquire) {
        STAGE.get()
    } else {
        None
    }
}

pub(crate) fn frame_open() -> bool {
    installed().is_some_and(|stage| stage.engine.lock().unwrap().phase == Phase::Rendering)
}

pub(crate) fn retire_inputs() -> bool {
    let Some(stage) = installed() else {
        return true;
    };
    let inputs = stage.engine.lock().unwrap().retire_inputs();
    let Ok(inputs) = inputs else { return false };
    // COM release stays outside the lock. Runtime owns its copied inputs.
    drop(inputs);
    true
}

fn identity(stage: &Stage, engine: &Engine, role: u32) -> Pass {
    let source = engine.source;
    let [width, height] = engine.render.extent;
    let rect = [0, 0, width as i32, height as i32];
    let main = if matches!(role, RSF_GAME_RENDER_FRAME | RSF_GAME_RENDER_WINDOW) {
        0
    } else {
        MAIN_VIEW
    };
    let decision = engine.policy.current();
    // The game ABI pass contains only scalars, pointers, and inline records with zero defaults.
    Pass {
        struct_size: size_of::<Pass>() as u32,
        role,
        session_id: stage.session,
        source_frame_id: source,
        submission_id: source,
        scope_id: source,
        family_key: main,
        view_key: main,
        pass_key: MAIN_VIEW,
        viewport_key: VIEWPORT,
        native_frame: u64::from(engine.render.native_frame),
        screen: decision.screen,
        flags: RSF_GAME_RENDER_PRIMARY
            | RSF_GAME_RENDER_AFTER_SIMULATION
            | if decision.reset != RSF_ANOMALY_RESET_NONE {
                RSF_GAME_RENDER_RESET
            } else {
                0
            },
        render_rect: rect,
        output_rect: rect,
        ..unsafe { mem::zeroed() }
    }
}

unsafe extern "C" fn bridge_log(_: Raw, message: *const c_char) {
    if let Some(stage) = STAGE.get()
        && !message.is_null()
    {
        stage.log.text(unsafe { CStr::from_ptr(message) });
    }
}

unsafe extern "C" fn before_present(_: Raw, chain: Raw) {
    if let Some((Some(sink), user)) = OVERLAY.get() {
        unsafe { sink(*user as Raw, chain) };
    }
}

fn matches_window(window: Option<&Pass>, source: u64, chain: Raw) -> bool {
    window.is_some_and(|window| window.source_frame_id == source && window.swapchain == chain)
}

unsafe extern "C" fn present_event(user: Raw, event: *const rsf_observer_present_event) {
    if event.is_null()
        || (unsafe { event.cast::<u32>().read() } as usize)
            < size_of::<rsf_observer_present_event>()
    {
        return;
    }
    let Some(stage) = STAGE.get() else { return };
    // Forward the checked event even when it doesn't belong to the adapter's window.
    unsafe { (stage.tables.fg.present_event)(user, event) };
    let event = unsafe { &*event };
    if event.flags & DXGI_PRESENT_TEST != 0 || !stage.owns(event.swapchain) {
        return;
    }
    if event.completed == 0 {
        stage.facade.store(event.swapchain, Ordering::Release);
    } else if event.result >= 0 {
        let mut engine = stage.engine.lock().unwrap();
        if engine.phase == Phase::Rendering
            && matches_window(engine.window.as_ref(), engine.source, event.swapchain)
        {
            engine.policy.presented();
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_fg_configure_ini(
    ini: *const u16,
    apply: bool,
    runtime: HMODULE,
    depth_rva: u32,
    log: rsf_game_log_fn,
    platform: *const Platform,
) -> bool {
    if ini.is_null() {
        return false;
    }
    let options = Options::read(ini);
    let selected = options.bridge;
    if selected && apply {
        Cold::configure(
            options,
            Log {
                callback: log,
                user: 0,
            },
        );
        install_runtime(runtime, depth_rva, platform);
    }
    selected
}

fn install_runtime(runtime: HMODULE, depth_rva: u32, platform: *const Platform) -> bool {
    let Some((options, log)) = Cold::attempt() else {
        return false;
    };
    let Some(platform) = (unsafe { EngineBindings::bind(platform) }) else {
        log.text(c"Anomaly FG refused: incomplete engine bindings.");
        return false;
    };
    Tables::query(runtime, log)
        .is_some_and(|tables| Stage::install(options, log, tables, platform, depth_rva))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_installed() -> bool {
    installed().is_some()
}

pub(crate) fn scope_hold(fallback: u32) -> u32 {
    installed().map_or_else(
        || fallback.clamp(1, 64),
        |stage| stage.engine.lock().unwrap().policy.scope_hold(),
    )
}

pub(crate) fn set_scope_hold(frames: u32) {
    if let Some(stage) = installed() {
        stage
            .engine
            .lock()
            .unwrap()
            .policy
            .set_scope_hold(frames.clamp(1, 64));
    }
}

// The overlay caller holds the NUL-terminated path through the reload.
pub(crate) unsafe fn reload_capture(ini: *const u16) {
    if let Some(stage) = installed() {
        let settings = unsafe { CaptureSettings::read(ini) };
        stage.options.capture.lock().unwrap().configure(settings);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_set_overlay_sink(sink: rsf_anomaly_fg_overlay_present, user: Raw) {
    let _ = OVERLAY.set((sink, user as usize));
}

pub(crate) fn overlay_options() -> Option<Frontend> {
    installed().map(|stage| stage.options.frontend)
}

pub(crate) fn foreign_chain(chain: Raw) -> bool {
    installed().is_some_and(|stage| {
        (unsafe { (stage.tables.present.has_owner)() != 0 }) && !stage.owns(chain)
    })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_fg_stage_camera(view: *const f32, projection: *const f32) {
    if let Some(stage) = installed()
        && !view.is_null()
        && !projection.is_null()
    {
        // SetCacheXform supplies two float[16] matrices for this call.
        let view = unsafe { view.cast::<[f32; 16]>().read_unaligned() };
        let projection = unsafe { projection.cast::<[f32; 16]>().read_unaligned() };
        stage.engine.lock().unwrap().camera_now = Camera::new(view, projection);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_input_begin(native_frame: u32) {
    let Some(stage) = installed() else { return };
    let (mut phase, source, started) = {
        let engine = stage.engine.lock().unwrap();
        (engine.phase, engine.source, engine.input_native_frame)
    };
    if phase == Phase::Rendering {
        return;
    }
    if phase == Phase::Open && started.is_some_and(|frame| frame != native_frame) {
        // A new input update closes the skipped source without observing its simulation end.
        stage.cpu(
            source,
            RSF_GAME_CPU_FRAME_END,
            false,
            0,
            0,
            RSF_GAME_CPU_RENDER_SKIPPED,
        );
        phase = Phase::Idle;
        stage.engine.lock().unwrap().phase = phase;
    }
    let source = if phase == Phase::Idle {
        stage.open_frame()
    } else {
        source
    };
    stage.engine.lock().unwrap().input_native_frame = Some(native_frame);
    // Repeated callbacks are native facts. The Runtime deduplicates them and owns pacing.
    stage.boundary(source, RSF_GAME_CPU_INPUT_SAMPLE);
    stage.boundary(source, RSF_GAME_CPU_SIMULATION_BEGIN);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_input_received(kinds: u32, message: u32) {
    let Some(stage) = installed() else { return };
    let (phase, source) = {
        let engine = stage.engine.lock().unwrap();
        (engine.phase, engine.source)
    };
    if kinds == 0 {
        let source = if phase == Phase::Idle { 0 } else { source };
        stage.cpu(
            source,
            RSF_GAME_CPU_INPUT_EVENT,
            false,
            0,
            message,
            RSF_GAME_CPU_WINDOW_MESSAGE,
        );
    } else if phase == Phase::Open {
        stage.cpu(source, RSF_GAME_CPU_INPUT_EVENT, true, kinds, message, 0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_render_begin(native_frame: u32, width: u32, height: u32) {
    let Some(stage) = installed() else { return };
    let phase = stage.engine.lock().unwrap().phase;
    if phase == Phase::Rendering {
        return;
    }
    if phase == Phase::Idle {
        stage.open_frame();
    }
    let (frame, submission, sr) = {
        let mut engine = stage.engine.lock().unwrap();
        let now = qpc();
        let frame_time = if engine.previous_begin != 0 && stage.frequency != 0 {
            ((now - engine.previous_begin) as f64 / stage.frequency as f64) as f32
        } else {
            0.0
        };
        engine.previous_begin = now;
        let extent = [width, height];
        engine.render = Render {
            native_frame,
            extent,
            camera: engine.camera_now,
            frame_time,
            invalidated: false,
        };
        let cut = engine
            .render
            .camera
            .zip(engine.camera_fed)
            .is_some_and(|(current, previous)| current.is_cut_from(previous));
        engine.policy.begin(native_frame, extent, cut);
        (
            identity(stage, &engine, RSF_GAME_RENDER_FRAME),
            identity(stage, &engine, RSF_GAME_RENDER_SUBMISSION),
            identity(stage, &engine, RSF_GAME_RENDER_SR),
        )
    };
    stage.boundary(frame.source_frame_id, RSF_GAME_CPU_SIMULATION_END);
    stage.pass(ptr::null_mut(), &frame, true);
    stage.pass(ptr::null_mut(), &submission, true);
    // Runtime SR work without an explicit source receives this frame's copied identity.
    stage.bind_source(Some(&sr));
    let mut engine = stage.engine.lock().unwrap();
    engine.window = None;
    engine.phase = Phase::Rendering;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_scope() {
    if let Some(stage) = installed() {
        let mut engine = stage.engine.lock().unwrap();
        if engine.phase == Phase::Rendering {
            engine.policy.scope();
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_ui_boundary() {
    let Some(stage) = installed() else { return };
    let (render, previous, sr, mut scene) = {
        let mut engine = stage.engine.lock().unwrap();
        if engine.phase != Phase::Rendering
            || engine.render.invalidated
            || engine.policy.ui_boundary().tag == 0
        {
            return;
        }
        (
            engine.render,
            engine.camera_fed,
            identity(stage, &engine, RSF_GAME_RENDER_SR),
            identity(stage, &engine, RSF_GAME_RENDER_FINAL_SCENE),
        )
    };
    let Some(chain) = stage.live_chain() else {
        stage.engine.lock().unwrap().policy.inputs(false);
        return;
    };
    let Some(context_lease) = (unsafe { stage.platform.graphics.immediate_context(chain) }) else {
        stage.engine.lock().unwrap().policy.inputs(false);
        return;
    };
    let context = context_lease.as_raw();
    let source = sr.source_frame_id;
    if !stage.inputs_ready(source) {
        stage.capture_adapter_inputs(context, sr, render, previous);
    }
    let inputs_ready = stage.inputs_ready(source);
    let Some(buffer) = (unsafe { stage.platform.graphics.backbuffer(chain) }) else {
        stage.engine.lock().unwrap().policy.inputs(false);
        return;
    };
    // The finished world scene, after phase_pp, tracers and scope copy, before CHUDManager::RenderUI.
    scene.scene_surface = buffer.as_raw();
    let began = stage.pass(context, &scene, true);
    let ended = stage.pass(context, &scene, false);
    let decision = stage
        .engine
        .lock()
        .unwrap()
        .policy
        .inputs(inputs_ready && began && ended);
    stage.maybe_dump(context, source, render.native_frame, decision);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_end_before() {
    let Some(stage) = installed() else { return };
    let (submission, mut window) = {
        let engine = stage.engine.lock().unwrap();
        if engine.phase != Phase::Rendering {
            return;
        }
        (
            identity(stage, &engine, RSF_GAME_RENDER_SUBMISSION),
            identity(stage, &engine, RSF_GAME_RENDER_WINDOW),
        )
    };
    stage.pass(ptr::null_mut(), &submission, false);
    stage.bind_source(None);
    if stage.engine.lock().unwrap().render.invalidated {
        return;
    }
    let Some(chain) = stage.live_chain() else {
        return;
    };
    let Some(output) = (unsafe { stage.platform.graphics.output_window(chain) }) else {
        return;
    };
    window.window_key = output as u64;
    window.rhi_viewport_key = RHI_VIEWPORT;
    window.swapchain = chain;
    if stage.pass(ptr::null_mut(), &window, true) {
        stage.engine.lock().unwrap().window = Some(window);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_end_after() {
    let Some(stage) = installed() else { return };
    let (window, frame) = {
        let mut engine = stage.engine.lock().unwrap();
        if engine.phase != Phase::Rendering {
            return;
        }
        (
            engine.window.take(),
            identity(stage, &engine, RSF_GAME_RENDER_FRAME),
        )
    };
    if let Some(window) = window {
        stage.pass(ptr::null_mut(), &window, false);
    }
    stage.pass(ptr::null_mut(), &frame, false);
    stage.boundary(frame.source_frame_id, RSF_GAME_CPU_FRAME_END);
    {
        let mut engine = stage.engine.lock().unwrap();
        if engine.policy.end().skip == RSF_ANOMALY_SKIP_NONE && engine.render.camera.is_some() {
            engine.camera_fed = engine.render.camera;
        }
    }
    stage.publish();
    stage.engine.lock().unwrap().phase = Phase::Idle;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_fg_stage_device_reset() {
    if let Some(stage) = installed() {
        let (window, inputs) = stage.engine.lock().unwrap().reset_device();
        // The graphics owner must not lend the old frame's identity to later SR work.
        stage.bind_source(None);
        if let Some(window) = window {
            stage.pass(ptr::null_mut(), &window, false);
        }
        // Release before the original CRender::reset_begin tears down the device/targets.
        drop(inputs);
    }
}

#[unsafe(export_name = "rsf_anomaly_fg_stage_report")]
pub(crate) extern "C" fn report(emit: rsf_game_log_fn, user: Raw) {
    let Some(stage) = installed().filter(|_| emit.is_some()) else {
        return;
    };
    let bridge = stage.tables.present;
    let generation = unsafe { (bridge.generation)() };
    let runtime = stage.runtime_status().unwrap_or_default();
    let vendor_presented = (runtime.available != 0
        && runtime.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0
        && unsafe { (bridge.generation)() } == generation)
        .then_some(PresentTotal {
            generation,
            count: runtime.vendor.total_presented,
        });
    let captures = stage.options.capture.lock().unwrap().total;
    let (now, previous) = {
        let mut engine = stage.engine.lock().unwrap();
        let now = Totals {
            policy: engine.policy.counters(),
            runtime_tagged: runtime.tagged_frames,
            vendor_presented,
            captures,
        };
        let previous = mem::replace(&mut engine.reported, now);
        (now, previous)
    };
    let (a, b) = (now.policy, previous.policy);
    let delta = u64::wrapping_sub;
    let skipped: [u64; 5] = std::array::from_fn(|i| delta(a.skipped[i], b.skipped[i]));
    let resets: [u64; 7] = std::array::from_fn(|i| delta(a.resets[i], b.resets[i]));
    let vendor_presents = now
        .vendor_presented
        .zip(previous.vendor_presented)
        .and_then(|(now, previous)| now.delta(previous))
        .map_or_else(|| "unavailable".to_owned(), |count| count.to_string());
    let log = Log {
        callback: emit,
        user: user as usize,
    };
    log.say(format!(
        "Anomaly FG window: source={} level={} eligible_at_ui_boundary={} fed={} presented={} unpresented={} scope_frames={} \
        skipped no_level={} scope={} no_present={} no_inputs={} resets first={} device={} discontinuity={} \
        scope_end={} size={} cut={} | runtime tagged={} vendor_presented={} captures={}",
        delta(a.source_frames, b.source_frames), delta(a.level_frames, b.level_frames),
        delta(a.eligible_at_ui_boundary, b.eligible_at_ui_boundary), delta(a.fed, b.fed), delta(a.presented, b.presented),
        delta(a.unpresented, b.unpresented), delta(a.scope_frames, b.scope_frames),
        skipped[1], skipped[2], skipped[3], skipped[4], resets[1], resets[2], resets[3], resets[4],
        resets[5], resets[6], delta(now.runtime_tagged, previous.runtime_tagged),
        vendor_presents, delta(now.captures, previous.captures),
    ));
    let (owner, backend, choices, switch) = unsafe {
        (
            (bridge.has_owner)(),
            (bridge.backend)(),
            (bridge.backend_choices)(),
            (bridge.switch_result)(),
        )
    };
    log.say(format!(
        "Anomaly FG state: owner={owner} backend={} choices=0x{choices:x} switch_result={switch} \
        requested_mode={} generated_requested={} effective_mode={} effective_generated={} active={} \
        reason={} last_result={} reflex={} pacing={} vendor_status={}",
        backend_name(backend), runtime.requested.mode, runtime.requested.generated_frames,
        runtime.vendor.effective_mode, runtime.vendor.effective_generated_frames, runtime.vendor.active,
        runtime.reason, runtime.last_result, runtime.vendor.effective_reflex,
        runtime.vendor.pacing_owner, runtime.vendor.vendor_status,
    ));
}

#[cfg(any(test, feature = "fg-fixture"))]
fn stage_status() -> Status {
    installed().map_or_else(Status::default, |stage| *stage.status.lock().unwrap())
}
