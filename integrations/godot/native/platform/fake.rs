// SPDX-License-Identifier: MIT
//! Test leaves. Recorded calls are thread-local, so parallel tests do not share them.
use super::{
    ABI_VERSION, FgFacts, FgInstall, FgOptions, InputFacts, LatencyFacts, LockedBody,
    PLATFORM_VERSION, Platform, Setup, Sharpening, SrContext, SrCreate, SrEvaluate, Status,
    VkFacts, VkLatencyFacts, VkProc, VkSrContext, VkSrCreate, WindowFacts, WindowIdentity,
};
use rsf_game_sys::{rsf_game_cpu_event, rsf_game_render_pass};
use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_void};
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
    assert_eq!(control.boot(&LEAVES, &setup(c"C:/game/runtime")), 1);
    control
}

pub(crate) fn boot_vulkan(vulkan: &crate::vulkan::Vulkan, runtime: &CStr) {
    assert_eq!(vulkan.boot(&LEAVES, &setup(runtime)), 1);
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
    pub(crate) static CAPTURE_SR_BEGIN: Cell<u32> = const { Cell::new(1) };
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
    pub(crate) static RENDER_BEGIN: Cell<u32> = const { Cell::new(1) };
    pub(crate) static END_SESSION: Cell<u32> = const { Cell::new(1) };
    /// The last Present pass: source frame, session, role, screen, swapchain and colour input.
    pub(crate) static WINDOW: Cell<Option<(u64, u64, u32, u32, usize, usize)>> =
        const { Cell::new(None) };
    /// The Vulkan SR context leaves in call order, each with its main argument.
    static VK_SR_CALLS: RefCell<Vec<(&'static str, u64)>> = const { RefCell::new(Vec::new()) };
    pub(crate) static VK_DRAIN: Cell<i32> = const { Cell::new(0) };
    pub(crate) static VK_IDLE: Cell<u32> = const { Cell::new(1) };
    pub(crate) static VK_DESTROY: Cell<i32> = const { Cell::new(0) };
    /// The next creation: its result and whether it returns a handle.
    pub(crate) static VK_CREATE: Cell<(i32, bool)> = const { Cell::new((0, true)) };
    static VK_CONTEXTS: Cell<usize> = const { Cell::new(0) };
    pub(crate) static VK_PLAN: Cell<(u32, u32)> = const { Cell::new((1280, 720)) };
    pub(crate) static VK_EVALUATED: Cell<i32> = const { Cell::new(0) };
    /// The command buffer an FSR4 evaluate continues on.
    pub(crate) static VK_NEXT: Cell<usize> = const { Cell::new(0x7000) };
    pub(crate) static VK_FALLBACK: Cell<i32> = const { Cell::new(0) };
    pub(crate) static VK_SHARPENING: Cell<Option<Sharpening>> = const { Cell::new(None) };
    /// A leaf that reports an exception.
    pub(crate) static VK_THROWS: Cell<Option<&'static str>> = const { Cell::new(None) };
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
        mode: 0,
        sharpness: 0.4,
        mas: 0,
        motion_sharpness: 0.2,
        motion_threshold: 0.0,
        motion_scale_limit: 10.0,
        contrast_enabled: 0,
        contrast: 0.0,
        depth_scale: 35.0,
        depth_bias: 0.001,
        clamp_output: 0,
        debug: 0,
    }
}

unsafe extern "C" fn tick_count() -> u64 {
    41
}

unsafe extern "C" fn prepare_capture() -> u32 {
    1
}

unsafe extern "C" fn fg_install(_: *const FgInstall) -> i32 {
    INSTALLS.set(INSTALLS.get() + 1);
    INSTALL_RESULT.get()
}

unsafe extern "C" fn fg_options(options: *const FgOptions) {
    // SAFETY: the owner passes a live record.
    FG_OPTIONS.set(Some(unsafe { *options }));
}

unsafe extern "C" fn dlss_preset_valid(preset: u32) -> u32 {
    u32::from(preset <= 15)
}

unsafe extern "C" fn sharpening_valid(options: *const Sharpening) -> u32 {
    // SAFETY: the owner passes a live record.
    u32::from(unsafe { (*options).mode } <= 4)
}

unsafe extern "C" fn write_sharpening_defaults(options: *mut Sharpening) {
    // SAFETY: the owner passes a writable record.
    unsafe { options.write(sharpening_defaults()) };
}

unsafe extern "C" fn fg_backend_known(backend: u32) -> u32 {
    u32::from(matches!(backend, 1 | 3 | 4 | 5))
}

unsafe extern "C" fn vulkan_supported() -> u32 {
    u32::from(VK_SUPPORTED.get())
}

unsafe extern "C" fn vulkan_latency_facts(facts: *mut VkLatencyFacts) -> u32 {
    match VK_LATENCY.get() {
        Some(value) => {
            // SAFETY: the owner passes a writable record.
            unsafe { facts.write(value) };
            1
        }
        None => 0,
    }
}

// A panic cannot leave these extern "C" leaves, so guard misuse is recorded as a call instead.
unsafe extern "C" fn vulkan_locked(body: LockedBody, user: *mut c_void) -> i32 {
    if VK_LOCKED.get() {
        VK_CALLS.with_borrow_mut(|calls| calls.push("reentered"));
        return 0;
    }
    VK_LOCKED.set(true);
    // SAFETY: the owner passes its body and closure.
    let result = unsafe { body(user) };
    VK_LOCKED.set(false);
    result
}

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

unsafe extern "C" fn vulkan_start(_: *const Setup) -> i32 {
    record("start");
    VK_START.get()
}

unsafe extern "C" fn vulkan_stop() -> i32 {
    record("stop");
    VK_STOP.get()
}

unsafe extern "C" fn vulkan_release(session: u64) {
    record("release");
    VK_RELEASED.set(Some(session));
}

unsafe extern "C" fn vulkan_facts(facts: *mut VkFacts) {
    record("facts");
    // SAFETY: the owner passes a writable record.
    unsafe { facts.write(VK_FACTS.get()) };
}

unsafe extern "C" fn vulkan_presenter_max(count: *mut u32) -> i32 {
    record("presenter_max");
    match VK_PRESENTER_MAX.get() {
        Some(value) => {
            // SAFETY: the owner passes a writable count.
            unsafe { count.write(value) };
            0
        }
        None => -8,
    }
}

unsafe extern "C" fn vulkan_fsr4_fg_probe() -> u32 {
    record("fsr4_fg_probe");
    VK_PROBE.get()
}

unsafe extern "C" fn vulkan_latency_options(mode: u32, frame_limit_us: u32) -> i32 {
    record("latency_options");
    VK_LATENCY_OPTIONS.set(Some((mode, frame_limit_us)));
    VK_LATENCY_RESULT.get()
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

unsafe extern "C" fn cpu_counters(facts: *mut InputFacts) {
    // SAFETY: the owner passes a writable record.
    unsafe { facts.write(INPUTS) };
}

unsafe extern "C" fn window_facts(window: *mut c_void, facts: *mut WindowFacts) {
    FACTS_WINDOW.set(Some(window.addr()));
    // SAFETY: the owner passes a writable record.
    unsafe {
        facts.write(WindowFacts {
            foreground_process_id: 10,
            current_process_id: 10,
            input_window_visible: 1,
            reserved: 0,
        })
    };
}

unsafe extern "C" fn fg_facts(facts: *mut FgFacts) {
    // SAFETY: the owner passes a writable record.
    unsafe { facts.write(FG.get()) };
}

unsafe extern "C" fn latency_facts(facts: *mut LatencyFacts) -> u32 {
    match LATENCY.get() {
        Some(value) => {
            // SAFETY: the owner passes a writable record.
            unsafe { facts.write(value) };
            1
        }
        None => 0,
    }
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

unsafe extern "C" fn d3d12_bind(_: *mut c_void, _: *mut c_void) -> u32 {
    d3d12("bind", 0);
    BIND.get()
}

unsafe extern "C" fn d3d12_wait(value: u64) -> u32 {
    d3d12("wait", value);
    WAIT.get()
}

unsafe extern "C" fn d3d12_signal(value: u64) -> u32 {
    d3d12("signal", value);
    SIGNAL.get()
}

unsafe extern "C" fn sr_default_backend() -> u32 {
    SR_DEFAULT.get()
}

unsafe extern "C" fn fg_default_backend() -> u32 {
    FG_DEFAULT.get()
}

unsafe extern "C" fn present_owner(backend: *mut u32) -> u32 {
    match PRESENT_OWNER.get() {
        Some(value) => {
            // SAFETY: the owner passes a writable backend.
            unsafe { backend.write(value) };
            1
        }
        None => 0,
    }
}

unsafe extern "C" fn present_request(backend: u32) {
    d3d12("present_request", backend.into());
}

unsafe extern "C" fn sr_create(request: *const SrCreate, context: *mut SrContext) -> u32 {
    // SAFETY: the owner passes live records.
    let (request, context) = unsafe { (&*request, &mut *context) };
    d3d12("create", request.backend.into());
    match REFUSED.get() {
        Some((backend, result)) if backend == request.backend => context.result = result,
        _ => {
            CONTEXTS.set(CONTEXTS.get() + 1);
            context.handle = ptr::without_provenance_mut(context_handle(CONTEXTS.get()) as usize);
            context.result = 0;
            (context.desired_width, context.desired_height) = PLAN.get();
        }
    }
    1
}

unsafe extern "C" fn sr_destroy(context: *mut c_void) {
    d3d12("destroy", context.addr() as u64);
}

unsafe extern "C" fn sr_evaluate(
    _: *mut c_void,
    _: *mut c_void,
    _: *const rsf_game_render_pass,
    request: *const SrEvaluate,
    result: *mut i32,
) -> u32 {
    // SAFETY: the owner passes live records.
    let request = unsafe { *request };
    d3d12("evaluate", request.slot.into());
    EVALUATE.set(Some(request));
    // SAFETY: the owner passes a writable result.
    unsafe { result.write(EVALUATED.get()) };
    1
}

unsafe extern "C" fn sr_refusal_log(_: *const rsf_game_render_pass, width: u32, _: u32) {
    d3d12("refusal_log", width.into());
}

unsafe extern "C" fn capture_sr_begin(_: *const rsf_game_render_pass) -> u32 {
    CAPTURE_SR_BEGIN.get()
}

unsafe extern "C" fn final_scene(_: *mut c_void, _: *const rsf_game_render_pass, slot: u32) -> u32 {
    d3d12("final_scene", slot.into());
    1
}

unsafe extern "C" fn fg_submitted(_: u64, frame: u64) {
    d3d12("fg_submitted", frame);
}

unsafe extern "C" fn render_begin(_: u64, frame: u64) -> u32 {
    d3d12("render_begin", frame);
    RENDER_BEGIN.get()
}

unsafe extern "C" fn present_window(pass: *const rsf_game_render_pass) {
    // SAFETY: the owner passes a live pass.
    let pass = unsafe { &*pass };
    WINDOW.set(Some((
        pass.source_frame_id,
        pass.session_id,
        pass.role,
        pass.screen,
        pass.swapchain.addr(),
        pass.color_input.addr(),
    )));
}

unsafe extern "C" fn fg_end_session(session: u64) -> u32 {
    d3d12("end_session", session);
    END_SESSION.get()
}

unsafe extern "C" fn d3d12_release(context: *mut c_void, _: u64) -> u32 {
    d3d12("release", context.addr() as u64);
    1
}

/// Records the call; zero when the test made it throw.
fn vk_sr(call: &'static str, argument: u64) -> u32 {
    VK_SR_CALLS.with_borrow_mut(|calls| calls.push((call, argument)));
    u32::from(VK_THROWS.get() != Some(call))
}

/// The Vulkan SR leaf calls since the last take.
pub(crate) fn take_vk_sr_calls() -> Vec<(&'static str, u64)> {
    VK_SR_CALLS.take()
}

/// The handle of the nth Vulkan SR context the fake created.
pub(crate) const fn vk_context_handle(nth: usize) -> u64 {
    (nth * 0x10000) as u64
}

unsafe extern "C" fn vulkan_presenter_drain(disable: u32) -> i32 {
    vk_sr("drain", disable.into());
    VK_DRAIN.get()
}

unsafe extern "C" fn vulkan_device_idle(device: *mut c_void, proc: Option<VkProc>) -> u32 {
    vk_sr("idle", device.addr() as u64);
    VK_IDLE.get() * u32::from(proc.is_some())
}

unsafe extern "C" fn vulkan_sr_destroy(context: *mut c_void) -> i32 {
    vk_sr("destroy", context.addr() as u64);
    VK_DESTROY.get()
}

unsafe extern "C" fn vulkan_fsr4_destroy(context: *mut c_void) -> i32 {
    vk_sr("fsr4_destroy", context.addr() as u64);
    0
}

fn vk_create(name: &'static str, request: *const VkSrCreate, created: *mut VkSrContext) -> u32 {
    // SAFETY: the owner passes live records.
    let (request, created) = unsafe { (&*request, &mut *created) };
    let completed = vk_sr(name, request.backend.into());
    let (result, handle) = VK_CREATE.get();
    if handle {
        VK_CONTEXTS.set(VK_CONTEXTS.get() + 1);
        created.handle = ptr::without_provenance_mut(vk_context_handle(VK_CONTEXTS.get()) as usize);
    }
    created.result = result;
    completed
}

unsafe extern "C" fn vulkan_sr_create(
    _: *mut c_void,
    request: *const VkSrCreate,
    created: *mut VkSrContext,
) -> u32 {
    vk_create("create", request, created)
}

unsafe extern "C" fn vulkan_fsr4_create(
    _: *mut c_void,
    request: *const VkSrCreate,
    created: *mut VkSrContext,
) -> u32 {
    vk_create("fsr4_create", request, created)
}

unsafe extern "C" fn vulkan_dlss_preset(_: *mut c_void, preset: u32) -> u32 {
    vk_sr("preset", preset.into())
}

fn vk_plan(name: &'static str, context: *mut c_void, width: *mut u32, height: *mut u32) -> u32 {
    let (planned_width, planned_height) = VK_PLAN.get();
    // SAFETY: the owner passes writable extents.
    unsafe {
        width.write(planned_width);
        height.write(planned_height);
    }
    vk_sr(name, context.addr() as u64)
}

unsafe extern "C" fn vulkan_sr_plan(
    _: *mut c_void,
    context: *mut c_void,
    width: *mut u32,
    height: *mut u32,
) -> u32 {
    vk_plan("plan", context, width, height)
}

unsafe extern "C" fn vulkan_fsr4_plan(
    _: *mut c_void,
    context: *mut c_void,
    width: *mut u32,
    height: *mut u32,
) -> u32 {
    vk_plan("fsr4_plan", context, width, height)
}

unsafe extern "C" fn vulkan_sr_evaluate(
    _: *mut c_void,
    context: *mut c_void,
    sharpening: *const Sharpening,
    result: *mut i32,
) -> u32 {
    // SAFETY: the owner passes live records.
    unsafe {
        VK_SHARPENING.set(Some(*sharpening));
        result.write(VK_EVALUATED.get());
    }
    vk_sr("evaluate", context.addr() as u64)
}

unsafe extern "C" fn vulkan_fsr4_evaluate(
    _: *mut c_void,
    context: *mut c_void,
    _: *const Sharpening,
    result: *mut i32,
    next: *mut *mut c_void,
) -> u32 {
    // SAFETY: the owner passes writable records.
    unsafe {
        result.write(VK_EVALUATED.get());
        next.write(ptr::without_provenance_mut(VK_NEXT.get()));
    }
    vk_sr("fsr4_evaluate", context.addr() as u64)
}

unsafe extern "C" fn vulkan_sr_fallback(_: *mut c_void, result: *mut i32) -> u32 {
    // SAFETY: the owner passes a writable result.
    unsafe { result.write(VK_FALLBACK.get()) };
    vk_sr("fallback", 0)
}

unsafe extern "C" fn vulkan_sr_log(_: *mut c_void, kind: u32, _: i32, _: i32) -> u32 {
    vk_sr("log", kind.into())
}

unsafe extern "C" fn vulkan_sr_dump_result(_: *mut c_void, backend: u32, _: i32) -> u32 {
    vk_sr("dump", backend.into())
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

unsafe extern "C" fn window_identity(window: *mut c_void, identity: *mut WindowIdentity) {
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
    // SAFETY: the owner passes a writable record.
    unsafe { identity.write(value) };
}

unsafe extern "C" fn window_procedure_get(_: *mut c_void) -> isize {
    window_call("get", 0);
    PROCEDURE.get()
}

unsafe extern "C" fn window_procedure_set(_: *mut c_void, procedure: isize) -> isize {
    window_call("set", procedure as u64);
    if PROCEDURE_REFUSED.get() {
        return 0;
    }
    PROCEDURE.replace(procedure)
}

unsafe extern "C" fn window_forward(
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

unsafe extern "C" fn query_counter(counter: *mut u64, frequency: *mut u64) {
    // SAFETY: the owner passes writable values.
    unsafe {
        counter.write(1000);
        frequency.write(10_000_000);
    }
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

unsafe extern "C" fn vulkan_present_marker(begin: u32) {
    window_call("marker", begin.into());
}

/// The last Auto question each policy leaf was asked: vendor, software and choices.
pub(crate) fn take_auto() -> (Option<(u32, u32, u32)>, Option<(u32, u32, u32)>) {
    (SR_ASKED.take(), FG_ASKED.take())
}

unsafe extern "C" fn sr_select_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    SR_ASKED.set(Some((vendor, software, choices)));
    SR_SELECTED.get()
}

unsafe extern "C" fn fg_select_backend(vendor: u32, software: u32, choices: u32) -> u32 {
    FG_ASKED.set(Some((vendor, software, choices)));
    FG_SELECTED.get()
}

pub(crate) static LEAVES: Platform = Platform {
    struct_size: size_of::<Platform>() as u32,
    version: PLATFORM_VERSION,
    tick_count,
    prepare_capture,
    fg_install,
    fg_options,
    dlss_preset_valid,
    sharpening_valid,
    sharpening_defaults: write_sharpening_defaults,
    fg_backend_known,
    vulkan_supported,
    vulkan_latency_facts,
    vulkan_locked,
    vulkan_start,
    vulkan_stop,
    vulkan_release,
    vulkan_facts,
    vulkan_presenter_max,
    vulkan_fsr4_fg_probe,
    vulkan_latency_options,
    cpu_counters,
    window_facts,
    fg_facts,
    latency_facts,
    d3d12_bind,
    d3d12_wait,
    d3d12_signal,
    sr_default_backend,
    fg_default_backend,
    present_owner,
    present_request,
    sr_create,
    sr_destroy,
    sr_evaluate,
    sr_refusal_log,
    capture_sr_begin,
    final_scene,
    fg_submitted,
    render_begin,
    present_window,
    fg_end_session,
    d3d12_release,
    vulkan_presenter_drain,
    vulkan_device_idle,
    vulkan_sr_destroy,
    vulkan_fsr4_destroy,
    vulkan_sr_create,
    vulkan_fsr4_create,
    vulkan_dlss_preset,
    vulkan_sr_plan,
    vulkan_fsr4_plan,
    vulkan_sr_evaluate,
    vulkan_fsr4_evaluate,
    vulkan_sr_fallback,
    vulkan_sr_log,
    vulkan_sr_dump_result,
    window_identity,
    window_procedure_get,
    window_procedure_set,
    window_forward,
    query_counter,
    cpu_event,
    vulkan_cpu_event,
    latency_ping,
    vulkan_latency_ping,
    vulkan_present_marker,
    sr_select_backend,
    fg_select_backend,
};
