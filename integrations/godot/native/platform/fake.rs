// SPDX-License-Identifier: MIT
//! Test leaves. Recorded calls are thread-local, so parallel tests do not share them.
use super::{
    ABI_VERSION, CpuService, FgFacts, FgInstall, FgOptions, InputFacts, LatencyFacts, Platform,
    Setup, Sharpening, SrContext, SrCreate, SrEvaluate, Status, VkFacts, VkLatencyFacts, VkProc,
    VkSrContext, VkSrCreate, WindowFacts, WindowIdentity,
};
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_int, c_void};
use std::ptr;

pub(crate) fn setup(runtime: &CStr) -> Setup {
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI_VERSION,
        runtime_directory: runtime.as_ptr(),
        engine_version: c"4.7.2".as_ptr(),
        project_id: c"125d2115-7176-46e1-8806-95b790f01717".as_ptr(),
        log: None,
        log_user: ptr::null_mut(),
    }
}

pub(crate) fn boot_d3d12() -> crate::Control {
    let control = crate::Control::new();
    INSTALL_RESULT.set(0);
    assert_eq!(control.boot(&setup(c"C:/game/runtime")), 1);
    control
}

pub(crate) fn boot_vulkan(vulkan: &crate::vulkan::Vulkan, runtime: &CStr) {
    assert_eq!(vulkan.boot(&setup(runtime)), 1);
}

pub(crate) fn status_header() -> Status {
    Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: ABI_VERSION,
        ..Status::default()
    }
}

thread_local! {
    pub(crate) static INSTALL_RESULT: Cell<i32> = const { Cell::new(0) };
    pub(crate) static INSTALLS: Cell<u32> = const { Cell::new(0) };
    pub(crate) static FG_OPTIONS: Cell<Option<FgOptions>> = const { Cell::new(None) };
    pub(crate) static FG: Cell<FgFacts> = Cell::new(FgFacts::default());
    pub(crate) static LATENCY: Cell<Option<LatencyFacts>> = const { Cell::new(None) };
    pub(crate) static VK_SUPPORTED: Cell<bool> = const { Cell::new(true) };
    pub(crate) static VK_LATENCY: Cell<Option<VkLatencyFacts>> = const { Cell::new(None) };
    static VK_LOCKED: Cell<bool> = const { Cell::new(false) };
    /// The guarded Vulkan leaves in call order.
    static VK_CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    pub(crate) static VK_START: Cell<i32> = const { Cell::new(1) };
    pub(crate) static VK_STOP: Cell<i32> = const { Cell::new(1) };
    pub(crate) static VK_RELEASED: Cell<Option<u64>> = const { Cell::new(None) };
    pub(crate) static VK_FACTS: Cell<VkFacts> = Cell::new(VkFacts::default());
    pub(crate) static VK_PRESENTER_MAX: Cell<Option<u32>> = const { Cell::new(None) };
    pub(crate) static VK_PROBE: Cell<u32> = const { Cell::new(0) };
    pub(crate) static VK_LATENCY_OPTIONS: Cell<Option<(u32, u32)>> = const { Cell::new(None) };
    pub(crate) static VK_LATENCY_RESULT: Cell<i32> = const { Cell::new(0) };
    /// The D3D12 leaves in call order, each with its main argument.
    static D3D12_CALLS: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
    pub(crate) static WAIT: Cell<u32> = const { Cell::new(1) };
    pub(crate) static BIND: Cell<u32> = const { Cell::new(1) };
    pub(crate) static SIGNAL: Cell<u32> = const { Cell::new(1) };
    pub(crate) static SR_SELECTED: Cell<u32> = const { Cell::new(3) };
    pub(crate) static FG_SELECTED: Cell<u32> = const { Cell::new(3) };
    pub(crate) static SR_DEFAULT: Cell<u32> = const { Cell::new(1) };
    pub(crate) static FG_DEFAULT: Cell<u32> = const { Cell::new(3) };
    pub(crate) static PRESENT_OWNER: Cell<Option<u32>> = const { Cell::new(None) };
    /// A provider whose context is not created, and the result its creation returns.
    pub(crate) static REFUSED: Cell<Option<(u32, i32)>> = const { Cell::new(None) };
    pub(crate) static PLAN: Cell<(u32, u32)> = const { Cell::new((1280, 720)) };
    static CONTEXTS: Cell<usize> = const { Cell::new(0) };
    pub(crate) static EVALUATED: Cell<i32> = const { Cell::new(0) };
    pub(crate) static EVALUATE: Cell<Option<SrEvaluate>> = const { Cell::new(None) };
    pub(crate) static END_SESSION: Cell<u32> = const { Cell::new(1) };
    /// The last Present pass: source frame, session, role, screen, swapchain and colour input.
    #[allow(clippy::type_complexity)]
    pub(crate) static WINDOW: Cell<Option<(u64, u64, u32, u32, usize, usize)>> =
        const { Cell::new(None) };
    /// The Vulkan SR context leaves in call order, each with its main argument.
    static VK_SR_CALLS: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
    pub(crate) static VK_DRAIN: Cell<i32> = const { Cell::new(0) };
    pub(crate) static VK_IDLE: Cell<u32> = const { Cell::new(1) };
    pub(crate) static VK_DESTROY: Cell<i32> = const { Cell::new(0) };
    /// The next creation: its result and whether it returns a handle.
    pub(crate) static VK_CREATE: Cell<(i32, bool)> = const { Cell::new((0, true)) };
    static VK_SPLIT_CONTEXTS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    pub(crate) static VK_SPLIT_OPEN_RESULT: Cell<i32> = const { Cell::new(0) };
    pub(crate) static VK_SPLIT_RECORD: Cell<bool> = const { Cell::new(true) };
    static VK_CONTEXTS: Cell<usize> = const { Cell::new(0) };
    pub(crate) static VK_PLAN: Cell<(u32, u32)> = const { Cell::new((1280, 720)) };
    pub(crate) static VK_EVALUATED: Cell<i32> = const { Cell::new(0) };
    /// The command buffer an FSR4 evaluate continues on.
    pub(crate) static VK_NEXT: Cell<usize> = const { Cell::new(0x7000) };
    pub(crate) static VK_FALLBACK: Cell<i32> = const { Cell::new(0) };
    pub(crate) static VK_SHARPENING: Cell<Option<Sharpening>> = const { Cell::new(None) };
    /// The window the last status facts described.
    pub(crate) static FACTS_WINDOW: Cell<Option<usize>> = const { Cell::new(None) };
    /// The identity every window reports: owner process, current process and class.
    pub(crate) static IDENTITY: Cell<(u32, u32, &'static str)> =
        const { Cell::new((10, 10, "Engine")) };
    /// The window procedure the fake window holds.
    pub(crate) static PROCEDURE: Cell<isize> = const { Cell::new(ENGINE_PROCEDURE) };
    /// SetWindowLongPtrW fails.
    pub(crate) static PROCEDURE_REFUSED: Cell<bool> = const { Cell::new(false) };
    /// The window, input, ping and marker leaves in call order, each with its main argument.
    static WINDOW_CALLS: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
    /// The CPU events reported: Vulkan, stage, session, message and flags.
    #[allow(clippy::type_complexity)]
    static CPU_EVENTS: RefCell<Vec<(bool, u32, u64, u32, u32)>> = const { RefCell::new(Vec::new()) };
    static SR_ASKED: Cell<Option<(u32, u32, u32)>> = const { Cell::new(None) };
    static FG_ASKED: Cell<Option<(u32, u32, u32)>> = const { Cell::new(None) };
}

/// The engine's own window procedure.
pub(crate) const ENGINE_PROCEDURE: isize = 0x5000;
/// What the forwarded procedure returns.
pub(crate) const FORWARDED: isize = 77;

/// `rsf_sharpening_defaults()` as `sharpening_settings.h` defines it.
pub(crate) fn sharpening_defaults() -> Sharpening {
    Sharpening {
        struct_size: size_of::<Sharpening>() as u32,
        abi_version: 1,
        mode: rsf_game_sys::RSF_SHARPENING_DEFAULT_MODE,
        sharpness: rsf_game_sys::RSF_SHARPENING_DEFAULT_SHARPNESS as f32,
        mas: rsf_game_sys::RSF_SHARPENING_DEFAULT_MAS,
        motion_sharpness: rsf_game_sys::RSF_SHARPENING_DEFAULT_MOTION_SHARPNESS as f32,
        motion_threshold: rsf_game_sys::RSF_SHARPENING_DEFAULT_MOTION_THRESHOLD as f32,
        motion_scale_limit: rsf_game_sys::RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT as f32,
        contrast_enabled: rsf_game_sys::RSF_SHARPENING_DEFAULT_CONTRAST_ENABLED,
        contrast: rsf_game_sys::RSF_SHARPENING_DEFAULT_CONTRAST as f32,
        depth_scale: rsf_game_sys::RSF_SHARPENING_DEFAULT_DEPTH_SCALE as f32,
        depth_bias: rsf_game_sys::RSF_SHARPENING_DEFAULT_DEPTH_BIAS as f32,
        clamp_output: rsf_game_sys::RSF_SHARPENING_DEFAULT_CLAMP_OUTPUT,
        debug: rsf_game_sys::RSF_SHARPENING_DEFAULT_DEBUG,
    }
}

/// The test platform: every method records its call or answers from the thread-local settings.
impl Platform {
    pub(crate) fn tick_count(&self) -> u64 {
        41
    }

    pub(crate) fn prepare_capture(&self) {}

    pub(crate) fn fg_install(&self, _: &FgInstall) -> i32 {
        INSTALLS.set(INSTALLS.get() + 1);
        INSTALL_RESULT.get()
    }

    pub(crate) fn fg_options(&self, options: &FgOptions) {
        FG_OPTIONS.set(Some(*options));
    }

    pub(crate) fn dlss_preset_valid(&self, preset: u32) -> bool {
        preset <= 15
    }

    pub(crate) fn sharpening_valid(&self, options: &Sharpening) -> bool {
        options.mode <= 4
    }

    pub(crate) fn sharpening_defaults(&self) -> Sharpening {
        sharpening_defaults()
    }

    pub(crate) fn fg_backend_known(&self, backend: u32) -> bool {
        matches!(backend, 1 | 3 | 4 | 5)
    }

    pub(crate) fn vulkan_supported(&self) -> bool {
        VK_SUPPORTED.get()
    }

    pub(crate) fn vulkan_latency_facts(&self) -> Option<VkLatencyFacts> {
        VK_LATENCY.get()
    }

    // A panic cannot be allowed to hide guard misuse, so it is recorded as a call instead.
    pub(crate) fn vulkan_locked<F: FnMut() -> c_int>(&self, mut body: F) -> c_int {
        if VK_LOCKED.get() {
            VK_CALLS.with_borrow_mut(|calls| calls.push("reentered"));
            return 0;
        }
        VK_LOCKED.set(true);
        let result = body();
        VK_LOCKED.set(false);
        result
    }

    pub(crate) fn vulkan_start(&self, _: &Setup) -> c_int {
        record("start");
        VK_START.get()
    }

    pub(crate) fn vulkan_stop(&self) -> c_int {
        record("stop");
        VK_STOP.get()
    }

    pub(crate) fn vulkan_release(&self, session: u64) {
        record("release");
        VK_RELEASED.set(Some(session));
    }

    pub(crate) fn vulkan_facts(&self) -> VkFacts {
        record("facts");
        VK_FACTS.get()
    }

    pub(crate) fn vulkan_presenter_max(&self) -> Option<u32> {
        record("presenter_max");
        VK_PRESENTER_MAX.get()
    }

    pub(crate) fn vulkan_fsr4_fg_probe(&self) -> u32 {
        record("fsr4_fg_probe");
        VK_PROBE.get()
    }

    pub(crate) fn vulkan_latency_options(&self, mode: u32, frame_limit_us: u32) -> i32 {
        record("latency_options");
        VK_LATENCY_OPTIONS.set(Some((mode, frame_limit_us)));
        VK_LATENCY_RESULT.get()
    }

    pub(crate) fn cpu_counters(&self) -> InputFacts {
        INPUTS
    }

    pub(crate) fn window_facts(&self, window: *mut c_void) -> WindowFacts {
        FACTS_WINDOW.set(Some(window.addr()));
        WindowFacts {
            foreground_process_id: 10,
            current_process_id: 10,
            input_window_visible: 1,
        }
    }

    pub(crate) fn fg_facts(&self) -> FgFacts {
        FG.get()
    }

    pub(crate) fn latency_facts(&self) -> Option<LatencyFacts> {
        LATENCY.get()
    }

    pub(crate) fn d3d12_bind(&self, _: *mut c_void, _: *mut c_void) -> bool {
        d3d12("bind", 0);
        BIND.get() != 0
    }

    pub(crate) fn d3d12_wait(&self, value: u64) -> bool {
        d3d12("wait", value);
        WAIT.get() != 0
    }

    pub(crate) fn d3d12_signal(&self, value: u64) -> bool {
        d3d12("signal", value);
        SIGNAL.get() != 0
    }

    pub(crate) fn sr_default_backend(&self) -> u32 {
        SR_DEFAULT.get()
    }

    pub(crate) fn fg_default_backend(&self) -> u32 {
        FG_DEFAULT.get()
    }

    pub(crate) fn present_owner(&self) -> Option<u32> {
        PRESENT_OWNER.get()
    }

    pub(crate) fn present_request(&self, backend: u32) {
        d3d12("present_request", backend.into());
    }

    pub(crate) fn sr_create(&self, request: &SrCreate, desired: (u32, u32)) -> Option<SrContext> {
        d3d12("create", request.backend.into());
        let mut context = SrContext {
            handle: ptr::null_mut(),
            result: 0,
            desired_width: desired.0,
            desired_height: desired.1,
        };
        match REFUSED.get() {
            Some((backend, result)) if backend == request.backend => context.result = result,
            _ => {
                CONTEXTS.set(CONTEXTS.get() + 1);
                context.handle =
                    ptr::without_provenance_mut(context_handle(CONTEXTS.get()) as usize);
                (context.desired_width, context.desired_height) = PLAN.get();
            }
        }
        Some(context)
    }

    pub(crate) fn sr_destroy(&self, context: *mut c_void) {
        d3d12("destroy", context.addr() as u64);
    }

    pub(crate) fn sr_evaluate(
        &self,
        _: *mut c_void,
        _: *mut c_void,
        _: &rsf_game_render_pass,
        request: &SrEvaluate,
    ) -> i32 {
        d3d12("evaluate", request.slot.into());
        EVALUATE.set(Some(*request));
        EVALUATED.get()
    }

    pub(crate) fn sr_refusal_log(&self, _: &rsf_game_render_pass, desired: (u32, u32)) {
        d3d12("refusal_log", desired.0.into());
    }

    pub(crate) fn capture_sr_begin(&self, _: &rsf_game_render_pass) {}

    pub(crate) fn final_scene(&self, _: *mut c_void, _: &rsf_game_render_pass, slot: u32) {
        d3d12("final_scene", slot.into());
    }

    pub(crate) fn fg_submitted(&self, _: u64, frame: u64) {
        d3d12("fg_submitted", frame);
    }

    pub(crate) fn render_begin(&self, _: u64, frame: u64) {
        d3d12("render_begin", frame);
    }

    pub(crate) fn present_window(&self, pass: &rsf_game_render_pass) {
        WINDOW.set(Some((
            pass.source_frame_id,
            pass.session_id,
            pass.role,
            pass.screen,
            pass.swapchain.addr(),
            pass.color_input.addr(),
        )));
    }

    pub(crate) fn fg_end_session(&self, session: u64) -> bool {
        d3d12("end_session", session);
        END_SESSION.get() != 0
    }

    pub(crate) fn d3d12_release(&self, context: *mut c_void, _: u64) {
        d3d12("release", context.addr() as u64);
    }

    pub(crate) fn vulkan_presenter_drain(&self, disable: bool) -> i32 {
        vk_sr("drain", disable.into());
        VK_DRAIN.get()
    }

    pub(crate) fn vulkan_device_idle(&self, device: *mut c_void, proc: Option<VkProc>) -> bool {
        vk_sr("idle", device.addr() as u64);
        VK_IDLE.get() != 0 && proc.is_some()
    }

    pub(crate) fn vulkan_sr_destroy(&self, context: *mut c_void) -> i32 {
        vk_sr(
            if vk_is_split(context) {
                "fsr4_destroy"
            } else {
                "destroy"
            },
            context.addr() as u64,
        );
        let result = VK_DESTROY.get();
        if result == 0 {
            VK_SPLIT_CONTEXTS
                .with_borrow_mut(|owners| owners.retain(|owner| *owner != context.addr()));
        }
        result
    }

    pub(crate) fn vulkan_sr_create(&self, _: *mut c_void, request: &VkSrCreate) -> VkSrContext {
        let mut created = VkSrContext {
            handle: ptr::null_mut(),
            result: 0,
        };
        let split = request.backend == 4;
        if split && VK_SPLIT_OPEN_RESULT.get() != 0 {
            created.result = VK_SPLIT_OPEN_RESULT.get();
            return created;
        }
        vk_sr(
            if split { "fsr4_create" } else { "create" },
            request.backend.into(),
        );
        let (result, handle) = VK_CREATE.get();
        if handle {
            VK_CONTEXTS.set(VK_CONTEXTS.get() + 1);
            created.handle =
                ptr::without_provenance_mut(vk_context_handle(VK_CONTEXTS.get()) as usize);
            if split {
                VK_SPLIT_CONTEXTS.with_borrow_mut(|owners| owners.push(created.handle.addr()));
            }
        }
        created.result = result;
        if result == 0 && request.backend == 1 {
            vk_sr("preset", request.preset.into());
        }
        created
    }

    pub(crate) fn vulkan_sr_plan(&self, _: *mut c_void, context: *mut c_void) -> (u32, u32) {
        vk_sr(
            if vk_is_split(context) {
                "fsr4_plan"
            } else {
                "plan"
            },
            context.addr() as u64,
        );
        VK_PLAN.get()
    }

    pub(crate) fn vulkan_sr_record(
        &self,
        _: *mut c_void,
        context: *mut c_void,
        sharpening: &Sharpening,
    ) -> (Option<i32>, i32, *mut c_void) {
        let mut commands = 0x20;
        let mut provider = None;
        if !context.is_null() {
            // The owner lends the resolved options for the evaluation it records.
            VK_SHARPENING.set(Some(*sharpening));
            let split = vk_is_split(context);
            let result = if split && !VK_SPLIT_RECORD.get() {
                -8
            } else {
                vk_sr(
                    if split { "fsr4_evaluate" } else { "evaluate" },
                    context.addr() as u64,
                );
                if split {
                    commands = VK_NEXT.get();
                }
                VK_EVALUATED.get()
            };
            provider = Some(result);
            if result == 0 {
                return (provider, 0, ptr::without_provenance_mut(commands));
            }
        }
        let next = ptr::without_provenance_mut(commands);
        if commands == 0 {
            return (provider, -8, next);
        }
        vk_sr("fallback", 0);
        (provider, VK_FALLBACK.get(), next)
    }

    pub(crate) fn vulkan_sr_log(&self, _: *mut c_void, kind: u32, _: i32, _: i32) {
        vk_sr("log", kind.into());
    }

    pub(crate) fn vulkan_sr_dump_result(&self, _: *mut c_void, backend: u32, _: i32) {
        vk_sr("dump", backend.into());
    }

    pub(crate) fn window_identity(&self, window: *mut c_void) -> WindowIdentity {
        window_call("identity", window.addr() as u64);
        let (process, current, class) = IDENTITY.get();
        let mut value = WindowIdentity {
            process_id: process,
            current_process_id: current,
            class_length: class.len() as u32,
            ..WindowIdentity::default()
        };
        for (slot, unit) in value.class_name.iter_mut().zip(class.encode_utf16()) {
            *slot = unit;
        }
        value
    }

    pub(crate) fn window_procedure_get(&self, _: *mut c_void) -> isize {
        window_call("get", 0);
        PROCEDURE.get()
    }

    pub(crate) fn window_procedure_set(&self, _: *mut c_void, procedure: isize) -> isize {
        window_call("set", procedure as u64);
        if PROCEDURE_REFUSED.get() {
            return 0;
        }
        PROCEDURE.replace(procedure)
    }

    pub(crate) fn window_forward(
        &self,
        previous: isize,
        window: *mut c_void,
        message: u32,
        _: usize,
        _: isize,
    ) -> isize {
        window_call("forward", previous as u64);
        window_call("forward_window", window.addr() as u64);
        window_call("forward_message", message.into());
        FORWARDED
    }

    pub(crate) fn query_counter(&self) -> (u64, u64) {
        (1000, 10_000_000)
    }

    pub(crate) fn cpu_service(&self, vulkan: bool) -> Option<&'static CpuService> {
        Some(if vulkan { &VK_CPU } else { &D3D12_CPU })
    }

    pub(crate) fn vulkan_present_marker(&self, begin: u32) {
        window_call("marker", begin.into());
    }

    pub(crate) fn sr_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        SR_ASKED.set(Some((vendor, software, choices)));
        SR_SELECTED.get()
    }

    pub(crate) fn fg_select_backend(&self, vendor: u32, software: u32, choices: u32) -> u32 {
        FG_ASKED.set(Some((vendor, software, choices)));
        FG_SELECTED.get()
    }
}

pub(crate) const INPUTS: InputFacts = InputFacts {
    keyboard: 1,
    mouse: 2,
    controller: 3,
    pings: 4,
    substitutes: 5,
    order_violations: 6,
    derived_boundaries: 7,
};

fn record(call: &'static str) {
    VK_CALLS.with_borrow_mut(|calls| {
        if !VK_LOCKED.get() {
            calls.push("unguarded");
        }
        calls.push(call);
    });
}

/// The guarded leaf calls since the last take.
pub(crate) fn take_calls() -> Vec<&'static str> {
    VK_CALLS.take()
}

fn d3d12(call: &'static str, argument: u64) {
    D3D12_CALLS.with_borrow_mut(|calls| calls.push((call, argument)));
}

/// The D3D12 leaf calls since the last take.
pub(crate) fn take_d3d12_calls() -> Vec<(&'static str, u64)> {
    D3D12_CALLS.take()
}

/// The handle of the nth context the fake created.
pub(crate) const fn context_handle(nth: usize) -> u64 {
    (nth * 0x1000) as u64
}

fn vk_sr(call: &'static str, argument: u64) {
    VK_SR_CALLS.with_borrow_mut(|calls| calls.push((call, argument)));
}

/// The Vulkan SR leaf calls since the last take.
pub(crate) fn take_vk_sr_calls() -> Vec<(&'static str, u64)> {
    VK_SR_CALLS.take()
}

/// The handle of the nth Vulkan SR context the fake created.
pub(crate) const fn vk_context_handle(nth: usize) -> u64 {
    (nth * 0x10000) as u64
}

pub(crate) fn vk_is_split(context: *mut c_void) -> bool {
    VK_SPLIT_CONTEXTS.with_borrow(|owners| owners.contains(&context.addr()))
}

fn window_call(call: &'static str, argument: u64) {
    WINDOW_CALLS.with_borrow_mut(|calls| calls.push((call, argument)));
}

/// The window leaf calls since the last take.
pub(crate) fn take_window_calls() -> Vec<(&'static str, u64)> {
    WINDOW_CALLS.take()
}

/// The CPU events since the last take.
pub(crate) fn take_cpu_events() -> Vec<(bool, u32, u64, u32, u32)> {
    CPU_EVENTS.take()
}

/// The last Auto question each policy leaf was asked: vendor, software and choices.
#[allow(clippy::type_complexity)]
pub(crate) fn take_auto() -> (Option<(u32, u32, u32)>, Option<(u32, u32, u32)>) {
    (SR_ASKED.take(), FG_ASKED.take())
}

fn cpu(vulkan: bool, event: *const rsf_game_cpu_event) {
    // SAFETY: the owner passes a live event.
    let event = unsafe { &*event };
    CPU_EVENTS.with_borrow_mut(|events| {
        events.push((
            vulkan,
            event.stage,
            event.session_id,
            event.message_id,
            event.flags,
        ))
    });
}

unsafe extern "C" fn cpu_event(event: *const rsf_game_cpu_event) {
    cpu(false, event);
}

unsafe extern "C" fn vulkan_cpu_event(event: *const rsf_game_cpu_event) {
    cpu(true, event);
}

unsafe extern "C" fn latency_ping(window: *mut c_void) -> i32 {
    window_call("ping", window.addr() as u64);
    1
}

unsafe extern "C" fn vulkan_latency_ping(window: *mut c_void) -> i32 {
    window_call("vulkan_ping", window.addr() as u64);
    2
}

pub(crate) static D3D12_CPU: CpuService = CpuService {
    struct_size: size_of::<CpuService>() as u32,
    version: super::CPU_SERVICE_VERSION,
    report: cpu_event,
    ping: latency_ping,
    execution: None,
};

unsafe extern "C" fn execution(pass: *const rsf_game_render_pass, begin: u32) -> i32 {
    // SAFETY: the adapter lends the pass until the call returns.
    let pass = unsafe { &*pass };
    window_call("execution_role", pass.role.into());
    window_call("execution_source", pass.source_frame_id);
    window_call("execution_window", pass.window_key);
    window_call("execution_begin", begin.into());
    0
}

pub(crate) static VK_CPU: CpuService = CpuService {
    report: vulkan_cpu_event,
    ping: vulkan_latency_ping,
    execution: Some(execution),
    ..D3D12_CPU
};
