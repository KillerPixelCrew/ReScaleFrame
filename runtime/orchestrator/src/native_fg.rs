// SPDX-License-Identifier: MIT
// Graphics-side owner of D3D11 frame generation through the D3D12 presentation bridge: the shared
// transfer device and fence, per-source input slots, the scene depth/motion capture and SR evaluation
// that fill them, HUD-less copies, provider admission at Prepare, retirement, Present observation and
// window-end sequencing, and the requested/observed FG status. The platform module owns native
// calls. native_fg_cpu.rs owns CPU pacing and latency markers.
use crate::com_ref::ComRef;
use crate::cpu_abi::{
    RSF_NATIVE_CPU_PRESENT_RETURNED, RSF_NATIVE_CPU_RESERVATION_BOUND,
    RSF_NATIVE_CPU_RESERVATION_CLOSED, rsf_native_cpu_frame as CpuFrame,
    rsf_native_cpu_reservation as Reservation,
};
use crate::fg_scene::{read_scene, set_scene, with_capture_scene};
use crate::fg_status;
use crate::native_cpu::{
    rendered_source, rsf_native_cpu_present_result, rsf_native_cpu_read,
    rsf_native_cpu_reservation_read,
};
use crate::native_fg_abi::*;
use crate::native_fg_cpu::{self as cpu_owner, Failure, Frame as CpuOwner};
use crate::render_abi::{
    RSF_FG_ABI_VERSION, RSF_FG_DYNAMIC, RSF_FG_FIXED, RSF_FG_OFF, RSF_PACING_NONE,
    RSF_PACING_REFLEX, RSF_REFLEX_BOOST, RSF_REFLEX_OFF, rsf_backend_log_fn as Log,
    rsf_backend_resource, rsf_dlss_frame as DlssFrame, rsf_fg_frame, rsf_fg_options,
    rsf_fg_status as ProviderStatus, rsf_generation_provider as Provider,
    rsf_native_fg_options as Options, rsf_native_fg_status as NativeStatus,
};
use crate::scene_abi::{
    RSF_NATIVE_SCENE_PRESENT_DIRECT, RSF_NATIVE_SCENE_PRESENT_NONE,
    RSF_NATIVE_SCENE_PRESENT_SAMPLED, rsf_native_scene_identity as SceneIdentity,
    rsf_observer_present_event as PresentEvent,
};
use rsf_game_sys::{
    RSF_FRAME_SOURCE_INPUT, RSF_FRAME_SOURCE_RENDERED, RSF_GAME_CAMERA_ABI_VERSION,
    RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_INPUT_SUBSTITUTE, RSF_GAME_FRAME_ABI_VERSION,
    RSF_GAME_RENDER_FG_INPUTS, RSF_GAME_RENDER_NO_FG, RSF_GAME_RENDER_RESET, rsf_frame_record,
    rsf_game_render_pass as Pass,
};
use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

mod platform;

use platform::NATIVE_PLATFORM;

type Handle = *mut c_void;

const OK: i32 = RSF_NFG_RESULT_OK as i32;
const NOT_READY: i32 = RSF_NFG_RESULT_NOT_READY;
const FEATURE_FAILED: i32 = RSF_NFG_RESULT_FEATURE_FAILED;
const SLOT_COUNT: usize = 6;
const PASS_SIZE: u32 = size_of::<Pass>() as u32;
const COMMON: u32 = RSF_NFG_STATE_COMMON;
const READ: u32 = RSF_NFG_STATE_NON_PIXEL_SHADER_RESOURCE;
const UAV: u32 = RSF_NFG_STATE_UNORDERED_ACCESS;
// Surfaces per slot: colour, depth, motion, exposure, SR output staging.
const COLOR: usize = 0;
const DEPTH: usize = 1;
const MOTION: usize = 2;
const EXPOSURE: usize = 3;
const STAGING: usize = 4;

// The private native table is fixed for the lifetime of the linked runtime.
struct Platform {
    cpu: &'static cpu_owner::Platform,
    present_has_owner: unsafe extern "C" fn() -> i32,
    present_is_owner: unsafe extern "C" fn(swapchain: *mut ::core::ffi::c_void) -> i32,
    present_host: unsafe extern "C" fn() -> *mut ::core::ffi::c_void,
    present_provider: unsafe extern "C" fn() -> *const rsf_generation_provider,
    present_session: unsafe extern "C" fn() -> *mut ::core::ffi::c_void,
    present_backend: unsafe extern "C" fn() -> u32,
    present_generation: unsafe extern "C" fn() -> u64,
    present_graphics: unsafe extern "C" fn(
        device12: *mut *mut ::core::ffi::c_void,
        queue12: *mut *mut ::core::ffi::c_void,
        native_device: *mut *mut ::core::ffi::c_void,
    ) -> i32,
    present_retire: unsafe extern "C" fn(
        provider: *mut ::core::ffi::c_void,
        list: *mut ::core::ffi::c_void,
        fence: *mut *mut ::core::ffi::c_void,
        value: *mut u64,
    ) -> i32,
    present_abort: unsafe extern "C" fn(id: u64) -> i32,
    host_token: unsafe extern "C" fn(host: *mut ::core::ffi::c_void, id: u64) -> i32,
    window_match: unsafe extern "C" fn(
        swapchain: *mut ::core::ffi::c_void,
        out: *mut rsf_game_render_pass,
    ) -> i32,
    scene_for_window: unsafe extern "C" fn(
        window: *const rsf_game_render_pass,
        out: *mut rsf_native_scene_identity,
    ) -> i32,
    scene_present_path: unsafe extern "C" fn(
        window: *const rsf_game_render_pass,
        scene: *const rsf_native_scene_identity,
    ) -> u32,
    allows_fg: unsafe extern "C" fn(record: *const rsf_frame_record) -> i32,
    context_device:
        unsafe extern "C" fn(context: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    query_context4:
        unsafe extern "C" fn(context: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    query_texture:
        unsafe extern "C" fn(object: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    texture_info: unsafe extern "C" fn(
        texture: *mut ::core::ffi::c_void,
        out: *mut rsf_nfg_texture_info,
    ) -> i32,
    create_event: unsafe extern "C" fn() -> *mut ::core::ffi::c_void,
    close_event: unsafe extern "C" fn(event: *mut ::core::ffi::c_void),
    fence_create: unsafe extern "C" fn(
        device11: *mut ::core::ffi::c_void,
        native_device: *mut ::core::ffi::c_void,
    ) -> *mut ::core::ffi::c_void,
    fence_d3d11: unsafe extern "C" fn(fence: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    fence_d3d12: unsafe extern "C" fn(fence: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    surface_create: unsafe extern "C" fn(
        device11: *mut ::core::ffi::c_void,
        native_device: *mut ::core::ffi::c_void,
        width: u32,
        height: u32,
        format: u32,
    ) -> *mut ::core::ffi::c_void,
    surface_destroy: unsafe extern "C" fn(surface: *mut ::core::ffi::c_void),
    surface_d3d11:
        unsafe extern "C" fn(surface: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    surface_d3d12:
        unsafe extern "C" fn(surface: *mut ::core::ffi::c_void) -> *mut ::core::ffi::c_void,
    fence_completed: unsafe extern "C" fn(fence12: *mut ::core::ffi::c_void) -> u64,
    wait_fence: unsafe extern "C" fn(
        fence12: *mut ::core::ffi::c_void,
        value: u64,
        event: *mut ::core::ffi::c_void,
    ) -> i32,
    flush: unsafe extern "C" fn(context4: *mut ::core::ffi::c_void),
    copy_resource: unsafe extern "C" fn(
        context4: *mut ::core::ffi::c_void,
        destination: *mut ::core::ffi::c_void,
        source: *mut ::core::ffi::c_void,
    ),
    context_signal: unsafe extern "C" fn(
        context4: *mut ::core::ffi::c_void,
        fence11: *mut ::core::ffi::c_void,
        value: u64,
    ) -> i32,
    context_wait: unsafe extern "C" fn(
        context4: *mut ::core::ffi::c_void,
        fence11: *mut ::core::ffi::c_void,
        value: u64,
    ) -> i32,
    create_commands: unsafe extern "C" fn(
        device12: *mut ::core::ffi::c_void,
        allocator: *mut *mut ::core::ffi::c_void,
        list: *mut *mut ::core::ffi::c_void,
    ) -> i32,
    create_output: unsafe extern "C" fn(
        device12: *mut ::core::ffi::c_void,
        width: u32,
        height: u32,
        format: u32,
    ) -> *mut ::core::ffi::c_void,
    reset_commands: unsafe extern "C" fn(
        allocator: *mut ::core::ffi::c_void,
        list: *mut ::core::ffi::c_void,
    ) -> i32,
    close_commands: unsafe extern "C" fn(list: *mut ::core::ffi::c_void) -> i32,
    execute: unsafe extern "C" fn(queue: *mut ::core::ffi::c_void, list: *mut ::core::ffi::c_void),
    transition: unsafe extern "C" fn(
        list: *mut ::core::ffi::c_void,
        resource: *mut ::core::ffi::c_void,
        before: u32,
        after: u32,
    ),
    copy_transitioned: unsafe extern "C" fn(
        list: *mut ::core::ffi::c_void,
        destination: *mut ::core::ffi::c_void,
        destination_before: u32,
        destination_after: u32,
        source: *mut ::core::ffi::c_void,
        source_before: u32,
        source_after: u32,
    ),
    queue_signal: unsafe extern "C" fn(
        queue: *mut ::core::ffi::c_void,
        fence12: *mut ::core::ffi::c_void,
        value: u64,
    ) -> i32,
    queue_wait: unsafe extern "C" fn(
        queue: *mut ::core::ffi::c_void,
        fence12: *mut ::core::ffi::c_void,
        value: u64,
    ) -> i32,
    dlss_evaluate: unsafe extern "C" fn(
        context: *mut ::core::ffi::c_void,
        frame: *const rsf_dlss_frame,
    ) -> i32,
    dlss_evaluate_shared: unsafe extern "C" fn(
        list: *mut ::core::ffi::c_void,
        frame: *const rsf_dlss_frame,
        source: u64,
    ) -> i32,
    dump_sr: unsafe extern "C" fn(
        context: *mut ::core::ffi::c_void,
        frame: *const rsf_dlss_frame,
        backend: u32,
    ),
    dump_fg: unsafe extern "C" fn(
        context: *mut ::core::ffi::c_void,
        pass: *const rsf_game_render_pass,
        frame: *const rsf_dlss_frame,
    ),
    dump_fg_prepare: unsafe extern "C" fn(
        pass: *const rsf_game_render_pass,
        frame: *const rsf_fg_frame,
        result: i32,
    ),
    dump_named: unsafe extern "C" fn(
        context: *mut ::core::ffi::c_void,
        texture: *mut ::core::ffi::c_void,
        directory: *const ::core::ffi::c_char,
        name: *const ::core::ffi::c_char,
        view: u32,
        scale: f32,
        log: rsf_backend_log_fn,
        log_user: *mut ::core::ffi::c_void,
    ) -> i32,
}

macro_rules! call {
    ($p:expr, $f:ident $(, $a:expr)* $(,)?) => {{
        // Every entry is present and takes the caller's validated or owned handles.
        #[allow(unused_unsafe)]
        let result =
            unsafe { ($p.$f)($($a),*) };
        result
    }};
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn now_qpc() -> u64 {
    let mut now = 0i64;
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut now) };
    now as u64
}

fn empty<T>() -> T {
    // Only used for the plain C structures of the bound ABI, for which all-zero is valid.
    unsafe { std::mem::zeroed() }
}

// Requested options, log sink and the pending/completed status the overlay and hosts read.
struct Settings {
    requested: Options,
    log: Log,
    log_user: Handle,
    cached: NativeStatus,
    pending_generation: u64,
    completed_generation: u64,
    completed: NativeStatus,
    // Whether the vendor was generating at the previous SR evaluation; SR history restarts only
    // when that changes.
    was_generating: bool,
    last_eligibility: u32,
    eligibility_lines: u32,
}

// The log user is an opaque value handed back to the sink.
unsafe impl Send for Settings {}

static SETTINGS: Mutex<Settings> = Mutex::new(Settings {
    requested: Options {
        struct_size: size_of::<Options>() as u32,
        mode: RSF_FG_OFF,
        generated_frames: 1,
        reflex_mode: RSF_REFLEX_OFF,
        frame_limit_us: 0,
        debug: 0,
    },
    log: None,
    log_user: ptr::null_mut(),
    cached: unsafe { std::mem::zeroed() },
    pending_generation: 0,
    completed_generation: 0,
    completed: unsafe { std::mem::zeroed() },
    was_generating: false,
    last_eligibility: u32::MAX,
    eligibility_lines: 0,
});

#[derive(Clone, Copy)]
pub(crate) struct Sink(Log, Handle);

impl Sink {
    pub(crate) fn available(self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn say(self, text: &str) {
        if let Some(log) = self.0
            && let Ok(text) = CString::new(text)
        {
            unsafe { log(self.1, text.as_ptr()) };
        }
    }
}

// Copy the sink under its lock before invoking the callback.
pub(crate) fn sink(debug_only: bool) -> Sink {
    let settings = lock(&SETTINGS);
    if debug_only && settings.requested.debug == 0 {
        Sink(None, ptr::null_mut())
    } else {
        Sink(settings.log, settings.log_user)
    }
}

pub(crate) fn requested() -> Options {
    lock(&SETTINGS).requested
}

fn stage(p: &Platform, name: &CStr, source: u64, native: u64, result: i32) {
    cpu_owner::stage(
        cpu_platform(p),
        call!(p, present_generation),
        &name.to_string_lossy(),
        source,
        native,
        result,
        None,
    );
}

fn trace(name: &CStr, source: u64, result: i32) {
    cpu_owner::trace(&name.to_string_lossy(), source, result);
}

fn cpu_platform(p: &Platform) -> &cpu_owner::Platform {
    // The native graphics callback table supplies a process-lifetime CPU platform.
    p.cpu
}

// The game's screen class decides, through the one rule every generation consumer applies.
fn generates_on(p: &Platform, screen: u32) -> bool {
    let mut record: rsf_frame_record = empty();
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.screen = screen;
    call!(p, allows_fg, &record) != 0
}

// A non-COM platform object released by its own destroy call.
struct Surface {
    handle: NonNull<c_void>,
    d3d11: Handle,
    d3d12: Handle,
    destroy: unsafe extern "C" fn(Handle),
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { (self.destroy)(self.handle.as_ptr()) };
    }
}

fn d3d11(surface: &Option<Surface>) -> Handle {
    surface.as_ref().map_or(ptr::null_mut(), |s| s.d3d11)
}

fn d3d12(surface: &Option<Surface>) -> Handle {
    surface.as_ref().map_or(ptr::null_mut(), |s| s.d3d12)
}

// Process-owned because CPU/vendor callbacks and presentation hooks remain installed. Explicit
// ownership transitions are cold-start only; no shutdown callback frees in-flight inputs.
struct Device {
    _device11: ComRef,
    context: ComRef,
    // Borrowed from the presentation bridge for the process lifetime.
    device12: Handle,
    queue: Handle,
    native_device: Handle,
    device11: Handle,
    // The shared fence and its event, never destroyed.
    fence11: Handle,
    fence12: Handle,
    event: Handle,
    serial: AtomicU64,
}

unsafe impl Send for Device {}
unsafe impl Sync for Device {}

static DEVICE: Mutex<Option<&'static Device>> = Mutex::new(None);

fn initialize(p: &Platform, context: Handle) -> Option<&'static Device> {
    let mut bound = lock(&DEVICE);
    if let Some(device) = *bound {
        return Some(device);
    }
    let (mut device12, mut queue, mut native_device) =
        (ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
    if call!(
        p,
        present_graphics,
        &mut device12,
        &mut queue,
        &mut native_device
    ) == 0
    {
        return None;
    }
    let device11 = unsafe { ComRef::from_owned(call!(p, context_device, context)) }?;
    let context4 = unsafe { ComRef::from_owned(call!(p, query_context4, context)) }?;
    let event = call!(p, create_event);
    if event.is_null() {
        return None;
    }
    let fence = call!(p, fence_create, device11.as_ptr(), native_device);
    if fence.is_null() {
        call!(p, close_event, event);
        return None;
    }
    let device = Box::leak(Box::new(Device {
        device11: device11.as_ptr(),
        _device11: device11,
        context: context4,
        device12,
        queue,
        native_device,
        fence11: call!(p, fence_d3d11, fence),
        fence12: call!(p, fence_d3d12, fence),
        event,
        serial: AtomicU64::new(0),
    }));
    *bound = Some(device);
    Some(device)
}

impl Device {
    // Queues the next serial on the D3D11 side. Unflushed: the presentation bridge's own Signal and
    // Flush later in the frame submit it, ahead of any CPU wait for slot reuse.
    fn signal11(&self, p: &Platform, serial: &mut u64) -> bool {
        *serial = self.serial.fetch_add(1, Ordering::Relaxed) + 1;
        call!(
            p,
            context_signal,
            self.context.as_ptr(),
            self.fence11,
            *serial
        ) != 0
    }

    fn surface(&self, p: &Platform, width: u32, height: u32, format: u32) -> Option<Surface> {
        let handle = NonNull::new(call!(
            p,
            surface_create,
            self.device11,
            self.native_device,
            width,
            height,
            format
        ))?;
        Some(Surface {
            d3d11: call!(p, surface_d3d11, handle.as_ptr()),
            d3d12: call!(p, surface_d3d12, handle.as_ptr()),
            destroy: p.surface_destroy,
            handle,
        })
    }
}

struct Commands {
    allocator: ComRef,
    list: ComRef,
}

fn create_commands(p: &Platform, device: &Device) -> Option<Commands> {
    let (mut allocator, mut list) = (ptr::null_mut(), ptr::null_mut());
    if call!(
        p,
        create_commands,
        device.device12,
        &mut allocator,
        &mut list
    ) == 0
    {
        return None;
    }
    Some(Commands {
        allocator: unsafe { ComRef::from_owned(allocator) }?,
        list: unsafe { ComRef::from_owned(list) }?,
    })
}

fn reset(p: &Platform, commands: &Commands) -> bool {
    call!(
        p,
        reset_commands,
        commands.allocator.as_ptr(),
        commands.list.as_ptr()
    ) != 0
}

struct Slot {
    source: u64,
    complete: u64,
    commands: Option<Commands>,
    retirement: Option<Commands>,
    // Prepare moved depth, motion and HUD-less colour to the read state; retire restores them.
    restore: bool,
    // SR-only storage (surfaces 0, 3 and 4 plus output) exists only once SR evaluated here.
    output: Option<ComRef>,
    surfaces: [Option<Surface>; 5],
    hudless: Option<Surface>,
    hudless_format: u32,
    hudless_width: u32,
    hudless_height: u32,
    hudless_ready: bool,
    width: u32,
    height: u32,
    output_width: u32,
    output_height: u32,
    generation: u32,
    motion_format: u32,
    // Optional translucency hints, each shaped like its own source.
    hints: [Option<Surface>; 5],
    hint_desc: [rsf_nfg_texture_info; 5],
    pass: Pass,
    prepared: bool,
    wait_ticks: u64,
    evaluate_ticks: u64,
}

impl Slot {
    const fn new() -> Self {
        Self {
            source: 0,
            complete: 0,
            commands: None,
            retirement: None,
            restore: false,
            output: None,
            surfaces: [const { None }; 5],
            hudless: None,
            hudless_format: 0,
            hudless_width: 0,
            hudless_height: 0,
            hudless_ready: false,
            width: 0,
            height: 0,
            output_width: 0,
            output_height: 0,
            generation: 0,
            motion_format: 0,
            hints: [const { None }; 5],
            hint_desc: unsafe { std::mem::zeroed() },
            pass: unsafe { std::mem::zeroed() },
            prepared: false,
            wait_ticks: 0,
            evaluate_ticks: 0,
        }
    }

    // The slot already holds inputs for exactly this pass: same source frame, session, view,
    // submission and native frame, the identity prepare checks against the presented scene.
    fn holds(&self, pass: &Pass) -> bool {
        self.source == pass.source_frame_id
            && self.pass.session_id == pass.session_id
            && self.pass.view_key == pass.view_key
            && self.pass.submission_id == pass.submission_id
            && self.pass.native_frame == pass.native_frame
    }
}

unsafe impl Send for Slot {}

// Vendor reads of a slot's inputs, written by retirement on the present thread.
struct Vendor {
    fence: Option<ComRef>,
    value: u64,
    // Retirement failed, so nothing says when the vendor stopped reading. Held until a later
    // retirement covers it or the provider generation changes (a drained transition).
    unknown: bool,
    generation: u64,
    // That failed retirement never ran the restore, so D3D12 still holds depth, motion and HUD-less
    // colour in the read state. resources() returns them to COMMON before reuse.
    read_state: bool,
}

impl Vendor {
    const fn new() -> Self {
        Self {
            fence: None,
            value: 0,
            unknown: false,
            generation: 0,
            read_state: false,
        }
    }
}

unsafe impl Send for Vendor {}

// Lock order: one slot, then VENDOR or SETTINGS. The render thread blocks on its slot; the present
// thread's Prepare only tries it and refuses generation when the render thread holds it.
static SLOTS: [Mutex<Slot>; SLOT_COUNT] = [const { Mutex::new(Slot::new()) }; SLOT_COUNT];
// SR uses private upload and command storage when the FG slot already owns explicit inputs.
static SR_SLOTS: [Mutex<Slot>; SLOT_COUNT] = [const { Mutex::new(Slot::new()) }; SLOT_COUNT];
static VENDOR: Mutex<[Vendor; SLOT_COUNT]> = Mutex::new([const { Vendor::new() }; SLOT_COUNT]);
#[derive(Clone, Copy, PartialEq, Eq)]
struct SourceTimeline {
    session: u64,
    generation: u64,
    viewport: u64,
    view: u64,
    chain: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct InputSample {
    timeline: SourceTimeline,
    source: u64,
    qpc: u64,
    frequency: u64,
}

impl InputSample {
    fn delta_from(self, previous: Self) -> Option<f32> {
        if self.timeline != previous.timeline
            || previous.source.checked_add(1) != Some(self.source)
            || previous.frequency != self.frequency
            || previous.qpc == 0
            || self.qpc <= previous.qpc
            || self.frequency == 0
        {
            return None;
        }
        let delta = (self.qpc - previous.qpc) as f64 * 1000.0 / self.frequency as f64;
        (delta <= 1000.0).then_some(delta as f32)
    }
}

#[derive(Clone, Copy, Default)]
struct InputTiming {
    last: Option<InputSample>,
    delta: Option<f32>,
}

impl InputTiming {
    fn observe(&mut self, sample: Option<InputSample>) -> Option<f32> {
        if self.last != sample {
            self.delta = sample
                .zip(self.last)
                .and_then(|(sample, previous)| sample.delta_from(previous));
            self.last = sample;
        }
        self.delta
    }
}

fn positive_delta(delta: f32) -> Option<f32> {
    (delta.is_finite() && delta > 0.0).then_some(delta)
}
static REFUSED_HUDLESS: Mutex<[u32; 4]> = Mutex::new([0; 4]);

thread_local! {
    static INPUT_TIMING: Cell<InputTiming> = const { Cell::new(InputTiming { last: None, delta: None }) };
    // Prepare's tagged slot, retired by the same present thread.
    static TAGGED: Cell<Option<usize>> = const { Cell::new(None) };
    static PRESENTING: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    // Source, provider generation and success of the last completed Present on this thread.
    static COMPLETED_PRESENT: Cell<(u64, u64, bool)> = const { Cell::new((0, 0, false)) };
}

// A completed Present's success is observed by at most one retirement.
fn consume_completed_present() -> (u64, u64, bool) {
    let (source, generation, ok) = COMPLETED_PRESENT.get();
    COMPLETED_PRESENT.set((source, generation, false));
    (source, generation, ok)
}

fn slot_index(source: u64) -> usize {
    (source % SLOT_COUNT as u64) as usize
}

fn slot(source: u64) -> MutexGuard<'static, Slot> {
    lock(&SLOTS[slot_index(source)])
}

fn try_slot(source: u64) -> Option<MutexGuard<'static, Slot>> {
    match SLOTS[slot_index(source)].try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn texture_info(p: &Platform, texture: Handle) -> rsf_nfg_texture_info {
    let mut info: rsf_nfg_texture_info = empty();
    call!(p, texture_info, texture, &mut info);
    info
}

// Returns surfaces a failed retirement left in the read state to COMMON on the interop queue, ahead
// of any D3D11 write. The vendor's reads have been joined or drained by now.
fn restore_read_state(p: &Platform, device: &Device, slot: &mut Slot) -> bool {
    // A list left open by a failed Close cannot be reset; replace the pair instead.
    if !slot.retirement.as_ref().is_some_and(|c| reset(p, c)) {
        slot.retirement = None;
        match create_commands(p, device) {
            Some(commands) if reset(p, &commands) => slot.retirement = Some(commands),
            _ => return false,
        }
    }
    let list = slot
        .retirement
        .as_ref()
        .map_or(ptr::null_mut(), |c| c.list.as_ptr());
    for surface in [&slot.surfaces[DEPTH], &slot.surfaces[MOTION], &slot.hudless] {
        if surface.is_some() {
            call!(p, transition, list, d3d12(surface), READ, COMMON);
        }
    }
    if call!(p, close_commands, list) == 0 {
        return false;
    }
    call!(p, execute, device.queue, list);
    // Waited on the CPU: rare, and the caller may destroy these surfaces right after.
    slot.complete = device.serial.fetch_add(1, Ordering::Relaxed) + 1;
    call!(p, queue_signal, device.queue, device.fence12, slot.complete) != 0
        && call!(p, wait_fence, device.fence12, slot.complete, device.event) != 0
}

// Joins the slot's previous use and sizes its storage. Generation keeps only depth and motion per
// slot; reconstruct adds what SR evaluation needs on top.
fn resources(
    p: &Platform,
    device: &Device,
    fg_index: Option<usize>,
    slot: &mut Slot,
    frame: &DlssFrame,
    reconstruct: bool,
) -> bool {
    let generation = call!(p, present_generation);
    let (vendor_fence, vendor_value, read_state) = if let Some(index) = fg_index {
        let mut vendor = lock(&VENDOR);
        let vendor = &mut vendor[index];
        if vendor.unknown {
            if vendor.generation == generation {
                return false;
            }
            // A drained provider switch ended every read of the old provider.
            vendor.unknown = false;
        }
        (vendor.fence.clone(), vendor.value, vendor.read_state)
    } else {
        (None, 0, false)
    };
    let started = now_qpc();
    // D3D11 signals are queued unflushed. Submit them before blocking on one, which happens when the
    // same frame reuses the slot, e.g. capture after a failed SR evaluation.
    if slot.complete != 0 && call!(p, fence_completed, device.fence12) < slot.complete {
        call!(p, flush, device.context.as_ptr());
    }
    let vendor_fence = vendor_fence
        .as_ref()
        .map_or(ptr::null_mut(), ComRef::as_ptr);
    if call!(p, wait_fence, device.fence12, slot.complete, device.event) == 0
        || call!(p, wait_fence, vendor_fence, vendor_value, device.event) == 0
    {
        return false;
    }
    slot.wait_ticks = now_qpc() - started;
    if slot.commands.is_none() {
        slot.commands = create_commands(p, device);
        if slot.commands.is_none() {
            return false;
        }
    }
    if slot.retirement.is_none() {
        slot.retirement = create_commands(p, device);
        if slot.retirement.is_none() {
            return false;
        }
    }
    if let Some(index) = fg_index.filter(|_| read_state) {
        if !restore_read_state(p, device, slot) {
            return false;
        }
        lock(&VENDOR)[index].read_state = false;
    }
    // Motion keeps its producer's precision: the transfer is a whole-resource copy.
    if frame.motion.is_null() {
        return false;
    }
    let motion = texture_info(p, frame.motion);
    if motion.format != RSF_NFG_FORMAT_RG16_FLOAT && motion.format != RSF_NFG_FORMAT_RG32_FLOAT {
        return false;
    }
    if slot.width != frame.render_width
        || slot.height != frame.render_height
        || slot.motion_format != motion.format
        || slot.output_width != frame.output_width
        || slot.output_height != frame.output_height
    {
        slot.surfaces = [const { None }; 5];
        slot.hints = [const { None }; 5];
        slot.output = None;
        slot.width = 0;
        slot.height = 0;
        let (width, height) = (frame.render_width, frame.render_height);
        slot.surfaces[DEPTH] = device.surface(p, width, height, RSF_NFG_FORMAT_R32_FLOAT);
        if slot.surfaces[DEPTH].is_none() {
            return false;
        }
        slot.surfaces[MOTION] = device.surface(p, width, height, motion.format);
        if slot.surfaces[MOTION].is_none() {
            return false;
        }
        slot.width = width;
        slot.height = height;
        slot.motion_format = motion.format;
        slot.output_width = frame.output_width;
        slot.output_height = frame.output_height;
        slot.generation = slot.generation.wrapping_add(1);
    }
    if !reconstruct || slot.output.is_some() {
        return true;
    }
    let shapes = [
        (
            COLOR,
            frame.render_width,
            frame.render_height,
            RSF_NFG_FORMAT_RGBA16_FLOAT,
        ),
        (EXPOSURE, 1, 1, RSF_NFG_FORMAT_R32_FLOAT),
        (
            STAGING,
            frame.output_width,
            frame.output_height,
            RSF_NFG_FORMAT_RGBA16_FLOAT,
        ),
    ];
    for (surface, width, height, format) in shapes {
        slot.surfaces[surface] = device.surface(p, width, height, format);
        if slot.surfaces[surface].is_none() {
            return false;
        }
    }
    slot.output = unsafe {
        ComRef::from_owned(call!(
            p,
            create_output,
            device.device12,
            frame.output_width,
            frame.output_height,
            RSF_NFG_FORMAT_RGBA16_FLOAT
        ))
    };
    slot.output.is_some()
}

// Colour formats the HUD-less copy can carry: D3D11-shareable display formats, SDR and HDR.
fn hudless_format(format: u32) -> bool {
    matches!(
        format,
        RSF_NFG_FORMAT_RGBA8_UNORM
            | RSF_NFG_FORMAT_RGBA8_UNORM_SRGB
            | RSF_NFG_FORMAT_BGRA8_UNORM
            | RSF_NFG_FORMAT_BGRA8_UNORM_SRGB
            | RSF_NFG_FORMAT_RGB10A2_UNORM
            | RSF_NFG_FORMAT_RGBA16_FLOAT
    )
}

fn depth_domains_bytes(pass: &Pass) -> &[u8] {
    let domains = &pass.depth_domains;
    unsafe { std::slice::from_raw_parts(ptr::from_ref(domains).cast::<u8>(), size_of_val(domains)) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_fg_options_set(options: *const Options) {
    if options.is_null() || unsafe { options.cast::<u32>().read() } < size_of::<Options>() as u32 {
        return;
    }
    let options = unsafe { options.read() };
    if options.mode > RSF_FG_DYNAMIC || options.reflex_mode > RSF_REFLEX_BOOST {
        return;
    }
    let mut settings = lock(&SETTINGS);
    if settings.requested.mode != options.mode
        || settings.requested.generated_frames != options.generated_frames
    {
        fg_status::deactivate(&mut settings.completed.vendor);
    }
    settings.requested = options;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_set_log(log: Log, user: Handle) {
    let mut settings = lock(&SETTINGS);
    settings.log = log;
    settings.log_user = user;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_fg_status_get(status: *mut NativeStatus) -> i32 {
    let p = &NATIVE_PLATFORM;
    if status.is_null() || unsafe { status.cast::<u32>().read() } < size_of::<NativeStatus>() as u32
    {
        return 0;
    }
    let generation = call!(p, present_generation);
    let owner = call!(p, present_has_owner) != 0;
    let rendered = rendered_source();
    let mut out = {
        let settings = lock(&SETTINGS);
        let mut out = fg_status::coherent(
            &settings.cached,
            settings.pending_generation,
            &settings.completed,
            settings.completed_generation,
            generation,
            owner,
            settings.requested.mode != RSF_FG_OFF,
        );
        out.requested = settings.requested;
        out
    };
    out.struct_size = size_of::<NativeStatus>() as u32;
    out.source_provenance = if rendered {
        RSF_FRAME_SOURCE_RENDERED
    } else {
        RSF_FRAME_SOURCE_INPUT
    };
    out.available = u32::from(owner);
    if !owner {
        out.vendor.active = 0;
        out.vendor.effective_mode = RSF_FG_OFF;
        out.vendor.effective_generated_frames = 0;
        out.vendor.low_latency_available = 0;
    }
    if owner && !rendered {
        cpu_owner::pacing_status(cpu_platform(p), &mut out.vendor);
    }
    if rendered {
        out.vendor.effective_reflex = RSF_REFLEX_OFF;
        out.vendor.low_latency_available = 0;
        out.vendor.latency_paced_frames = 0;
        out.vendor.valid_statistics &= !RSF_NFG_STAT_LATENCY_AVAILABLE;
    }
    unsafe { status.write(out) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_fg_scene(pass: *const Pass) {
    let p = &NATIVE_PLATFORM;
    unsafe { set_scene(pass, |pass| refuse_held_inputs(p, pass)) };
}

fn refuse_held_inputs(p: &Platform, pass: &Pass) {
    if call!(p, present_has_owner) != 0 {
        let mut held = slot(pass.source_frame_id);
        if held.prepared && held.holds(pass) {
            held.pass.flags |= RSF_GAME_RENDER_NO_FG;
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_capture_scoped(
    context: Handle,
    identity: *const Pass,
    frame: *const DlssFrame,
) {
    if context.is_null()
        || identity.is_null()
        || frame.is_null()
        || unsafe { identity.cast::<u32>().read() } < PASS_SIZE
        || (unsafe { frame.cast::<u32>().read() } as usize) < size_of::<DlssFrame>()
    {
        return;
    }
    let p = &NATIVE_PLATFORM;
    with_capture_scene(
        unsafe { &*identity },
        |pass| refuse_held_inputs(p, pass),
        || capture(p, context, frame),
    );
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_final(context: Handle, pass: *const Pass) {
    let Some(pass) = (unsafe { pass.as_ref() }) else {
        return;
    };
    let p = &NATIVE_PLATFORM;
    if context.is_null()
        || pass.scene_surface.is_null()
        || pass.source_frame_id == 0
        || call!(p, present_has_owner) == 0
    {
        return;
    }
    // HUD-less colour only feeds generation: no copy while it is off or no provider is live.
    if requested().mode == RSF_FG_OFF
        || call!(p, present_provider).is_null()
        || call!(p, present_session).is_null()
    {
        return;
    }
    let (source, native) = (pass.source_frame_id, pass.native_frame);
    stage(p, c"final-enter", source, native, 0);
    let Some(device) = initialize(p, context) else {
        return;
    };
    let mut slot = slot(source);
    if !slot.prepared || !slot.holds(pass) {
        return;
    }
    slot.hudless_ready = false;
    let Some(texture) =
        (unsafe { ComRef::from_owned(call!(p, query_texture, pass.scene_surface)) })
    else {
        return;
    };
    let desc = texture_info(p, texture.as_ptr());
    if desc.width != slot.output_width
        || desc.height != slot.output_height
        || desc.samples != 1
        || !hudless_format(desc.format)
    {
        // Generation then waits with reason HUDLESS_MISSING; say once per surface shape why.
        let shape = [desc.width, desc.height, desc.format, desc.samples];
        let changed = {
            let mut refused = lock(&REFUSED_HUDLESS);
            let changed = *refused != shape;
            *refused = shape;
            changed
        };
        if changed {
            sink(false).say(&format!(
                "FG HUD-less surface refused: {}x{} format {} samples {}, output {}x{}",
                desc.width,
                desc.height,
                desc.format,
                desc.samples,
                slot.output_width,
                slot.output_height
            ));
        }
        return;
    }
    if slot.hudless.is_none()
        || slot.hudless_format != desc.format
        || slot.hudless_width != desc.width
        || slot.hudless_height != desc.height
    {
        slot.hudless_width = 0;
        slot.hudless_height = 0;
        slot.hudless = None;
        slot.hudless = device.surface(p, desc.width, desc.height, desc.format);
        if slot.hudless.is_none() {
            return;
        }
        slot.hudless_format = desc.format;
        slot.hudless_width = desc.width;
        slot.hudless_height = desc.height;
    }
    stage(p, c"final-copy", source, native, 0);
    call!(
        p,
        copy_resource,
        device.context.as_ptr(),
        d3d11(&slot.hudless),
        texture.as_ptr()
    );
    let mut complete = 0;
    slot.hudless_ready = device.signal11(p, &mut complete);
    slot.complete = complete;
    let result = if slot.hudless_ready { OK } else { NOT_READY };
    stage(p, c"final-done", source, native, result);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_capture(context: Handle, frame: *const DlssFrame) {
    let p = &NATIVE_PLATFORM;
    capture(p, context, frame);
}

fn capture(p: &Platform, context: Handle, frame: *const DlssFrame) {
    let scene = read_scene();
    let frame = unsafe { frame.as_ref() };
    if let Some(frame) = frame
        && !context.is_null()
        && frame.camera_motion_included != 0
    {
        call!(p, dump_fg, context, &scene, frame);
    }
    let Some(frame) = frame else {
        return;
    };
    if call!(p, present_has_owner) == 0
        || context.is_null()
        || scene.source_frame_id == 0
        || frame.camera_motion_included == 0
        || requested().mode == RSF_FG_OFF
    {
        return;
    }
    let Some(device) = initialize(p, context) else {
        return;
    };
    let (source, native) = (scene.source_frame_id, scene.native_frame);
    let index = slot_index(source);
    let mut slot = slot(source);
    // Reuse matching explicit inputs without recopying. A fallback SR capture cannot supply
    // admission for, or replace, the producer's explicit lease.
    if slot.prepared && slot.holds(&scene) {
        if scene.role != RSF_GAME_RENDER_FG_INPUTS {
            return;
        }
        if slot.pass.role == RSF_GAME_RENDER_FG_INPUTS {
            // A repeated explicit lease can refuse this source, never erase an earlier refusal. SR
            // metadata is not an FG admission authority.
            slot.pass.flags |= scene.flags & (RSF_GAME_RENDER_RESET | RSF_GAME_RENDER_NO_FG);
            if depth_domains_bytes(&slot.pass) != depth_domains_bytes(&scene) {
                slot.pass.flags |= RSF_GAME_RENDER_NO_FG;
            }
            return;
        }
        // Explicit inputs supersede a fallback SR capture, including its contents.
    }
    stage(p, c"capture-resources", source, native, 0);
    if frame.depth.is_null()
        || frame.motion.is_null()
        || !resources(p, device, Some(index), &mut slot, frame, false)
    {
        return;
    }
    stage(p, c"capture-copy", source, native, 0);
    slot.prepared = false;
    let context4 = device.context.as_ptr();
    call!(
        p,
        copy_resource,
        context4,
        d3d11(&slot.surfaces[DEPTH]),
        frame.depth
    );
    call!(
        p,
        copy_resource,
        context4,
        d3d11(&slot.surfaces[MOTION]),
        frame.motion
    );
    let mut complete = 0;
    let signalled = device.signal11(p, &mut complete);
    slot.complete = complete;
    if !signalled {
        return;
    }
    slot.source = source;
    slot.pass = scene;
    slot.prepared = true;
    slot.hudless_ready = false;
    cpu_owner::count_sr(source);
    stage(p, c"capture-done", source, native, 0);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_evaluate(context: Handle, frame: *const DlssFrame) -> i32 {
    let p = &NATIVE_PLATFORM;
    let started = now_qpc();
    let host = call!(p, present_host);
    if host.is_null() {
        call!(p, dump_sr, context, frame, RSF_NFG_SR_DLSS);
        return call!(p, dlss_evaluate, context, frame);
    }
    let scene = read_scene();
    let frame = unsafe { frame.as_ref() };
    let missing = if context.is_null() {
        1
    } else if frame.is_none() {
        2
    } else if scene.source_frame_id == 0 {
        4
    } else if frame.is_some_and(|f| f.camera_motion_included == 0) {
        8
    } else {
        0
    };
    let failure = |stage, result, detail| {
        cpu_owner::sr_failure(
            stage,
            scene.source_frame_id,
            scene.session_id,
            result,
            detail,
        );
    };
    let Some(frame) = frame.filter(|_| missing == 0) else {
        failure(Failure::SrPrerequisite, NOT_READY, missing);
        return NOT_READY;
    };
    let source = scene.source_frame_id;
    if call!(p, host_token, host, source) == 0 {
        failure(Failure::SrToken, NOT_READY, 0);
        return NOT_READY;
    }
    cpu_owner::count_sr(source);
    let Some(device) = initialize(p, context) else {
        failure(Failure::SrInitialize, NOT_READY, 0);
        return NOT_READY;
    };
    let index = slot_index(source);
    let fg_slot = slot(source);
    // A secondary SR view of this source also has no authority to replace its explicit FG lease.
    let explicit_inputs = fg_slot.prepared
        && fg_slot.source == source
        && fg_slot.pass.session_id == scene.session_id
        && fg_slot.pass.role == RSF_GAME_RENDER_FG_INPUTS;
    let mut slot = if explicit_inputs {
        drop(fg_slot);
        lock(&SR_SLOTS[index])
    } else {
        fg_slot
    };
    // A failed SR evaluation cannot keep an inferred FG lease eligible after overwriting it.
    slot.prepared = false;
    let fg_index = (!explicit_inputs).then_some(index);
    if !resources(p, device, fg_index, &mut slot, frame, true) {
        return FEATURE_FAILED;
    }
    let Some(commands) = slot.commands.as_ref().filter(|c| reset(p, c)) else {
        return FEATURE_FAILED;
    };
    let list = commands.list.as_ptr();
    let mut translated = *frame;
    // Dense motion has no unwritten sentinel. Valid zero displacement must remain valid.
    translated.motion_invalid_value = f32::MAX;
    let allowed = generates_on(p, scene.screen);
    {
        // DLSS-G shares these constants. History restarts when generation starts or stops, not on
        // every frame it stays off (no provider, an ineligible frame, VSync unsupported).
        let mut settings = lock(&SETTINGS);
        let generating = settings.cached.vendor.effective_mode != RSF_FG_OFF;
        if settings.requested.mode != RSF_FG_OFF && allowed && generating != settings.was_generating
        {
            translated.reset = 1;
        }
        settings.was_generating = generating;
    }
    call!(p, dump_sr, context, &translated, RSF_NFG_SR_DLSS);
    let context4 = device.context.as_ptr();
    let inputs = [frame.color_in, frame.depth, frame.motion, frame.exposure];
    let mut targets = [ptr::null_mut(); 4];
    for (i, input) in inputs.into_iter().enumerate() {
        if input.is_null() {
            continue;
        }
        call!(p, copy_resource, context4, d3d11(&slot.surfaces[i]), input);
        targets[i] = d3d12(&slot.surfaces[i]);
        call!(p, transition, list, targets[i], COMMON, READ);
    }
    [
        translated.color_in,
        translated.depth,
        translated.motion,
        translated.exposure,
    ] = targets;
    let hint_inputs = [
        frame.color_before_transparency,
        frame.transparency_layer,
        frame.reactive_mask,
        frame.transparency_hint,
        frame.bias_current_color,
    ];
    let mut hint_targets = [ptr::null_mut(); 5];
    for (i, input) in hint_inputs.into_iter().enumerate() {
        if input.is_null() {
            continue;
        }
        let desc = texture_info(p, input);
        let known = slot.hint_desc[i];
        if slot.hints[i].is_none()
            || known.width != desc.width
            || known.height != desc.height
            || known.format != desc.format
        {
            slot.hint_desc[i] = empty();
            // A hint that cannot cross is left untagged rather than failing the reconstruction.
            slot.hints[i] = None;
            slot.hints[i] = device.surface(p, desc.width, desc.height, desc.format);
            if slot.hints[i].is_none() {
                continue;
            }
            slot.hint_desc[i] = desc;
        }
        call!(p, copy_resource, context4, d3d11(&slot.hints[i]), input);
        hint_targets[i] = d3d12(&slot.hints[i]);
        call!(p, transition, list, hint_targets[i], COMMON, READ);
    }
    [
        translated.color_before_transparency,
        translated.transparency_layer,
        translated.reactive_mask,
        translated.transparency_hint,
        translated.bias_current_color,
    ] = hint_targets;
    translated.motion_depth_layer = ptr::null_mut(); // Consumed by the D3D11 resolve already.
    let output = slot.output.as_ref().map_or(ptr::null_mut(), ComRef::as_ptr);
    translated.color_out = output;
    // This Flush stays: the D3D12 queue waits on the signal below within this frame.
    let mut uploaded = 0;
    if !device.signal11(p, &mut uploaded) {
        return FEATURE_FAILED;
    }
    call!(p, flush, context4);
    if call!(p, queue_wait, device.queue, device.fence12, uploaded) == 0 {
        return FEATURE_FAILED;
    }
    let result = call!(p, dlss_evaluate_shared, list, &translated, source);
    if result == NOT_READY {
        failure(Failure::SrSharedEvaluate, result, 0);
    }
    if result == OK {
        let staging = d3d12(&slot.surfaces[STAGING]);
        call!(
            p,
            copy_transitioned,
            list,
            staging,
            COMMON,
            COMMON,
            output,
            UAV,
            UAV
        );
    }
    // SR only borrows colour/exposure. FG retains immutable depth and motion in the slot.
    for target in targets.into_iter().chain(hint_targets) {
        if !target.is_null() {
            call!(p, transition, list, target, READ, COMMON);
        }
    }
    if call!(p, close_commands, list) == 0 {
        return FEATURE_FAILED;
    }
    call!(p, execute, device.queue, list);
    slot.complete = device.serial.fetch_add(1, Ordering::Relaxed) + 1;
    if call!(p, queue_signal, device.queue, device.fence12, slot.complete) == 0
        || call!(p, context_wait, context4, device.fence11, slot.complete) == 0
    {
        return FEATURE_FAILED;
    }
    if result == OK {
        let staging = d3d11(&slot.surfaces[STAGING]);
        call!(p, copy_resource, context4, frame.color_out, staging);
    }
    // The D3D11 output consumer also belongs to this lease. A D3D12 evaluation fence alone cannot
    // authorize destroying/reallocating shared storage while this copy is outstanding.
    let mut complete = 0;
    let signalled = device.signal11(p, &mut complete);
    slot.complete = complete;
    if !signalled {
        return FEATURE_FAILED;
    }
    if !explicit_inputs {
        slot.source = source;
        slot.pass = scene;
        slot.prepared = result == OK;
        slot.hudless_ready = false;
    }
    slot.evaluate_ticks = now_qpc() - started;
    result
}

// Admission facts for one Prepare, logged as one row and folded into the eligibility mask.
struct Admission {
    window_matched: bool,
    scene_matched: bool,
    presentation_path: u32,
    matched: bool,
    inputs: bool,
    hudless: bool,
    submitted: bool,
    identity: bool,
    scene_supported: bool,
    continuous: bool,
    vsync: bool,
    cpu_valid: bool,
    timing_valid: bool,
    rendered: bool,
    no_pacer: bool,
    reserved_bound: bool,
}

impl Admission {
    fn valid(&self) -> bool {
        self.matched
            && self.inputs
            && self.hudless
            && self.submitted
            && self.identity
            && self.scene_supported
            && self.continuous
            && self.vsync
            && self.cpu_valid
            && self.timing_valid
    }

    fn reason(&self) -> u32 {
        use crate::fg_reasons::codes::*;
        if !self.matched {
            RSF_NATIVE_FG_REASON_NO_MATCH
        } else if !self.inputs {
            RSF_NATIVE_FG_REASON_NOT_PREPARED
        } else if !self.submitted {
            RSF_NATIVE_FG_REASON_NO_SUBMISSION
        } else if !self.scene_supported {
            RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED
        } else if !self.continuous {
            RSF_NATIVE_FG_REASON_RESET
        } else if !self.vsync {
            RSF_NATIVE_FG_REASON_VSYNC
        } else if !self.hudless {
            RSF_NATIVE_FG_REASON_HUDLESS_MISSING
        } else if !self.valid() {
            RSF_NATIVE_FG_REASON_INVALID_INPUT
        } else {
            RSF_NATIVE_FG_REASON_NONE
        }
    }

    fn eligibility(&self, screen: u32) -> u32 {
        u32::from(self.window_matched)
            | (u32::from(self.scene_matched) << 1)
            | (u32::from(self.matched) << 2)
            | (u32::from(self.inputs) << 3)
            | (u32::from(self.submitted) << 4)
            | (u32::from(self.identity) << 5)
            | (u32::from(self.scene_supported) << 6)
            | (u32::from(self.continuous) << 7)
            | (u32::from(self.vsync) << 8)
            | (u32::from(self.cpu_valid) << 9)
            | (self.presentation_path << 10)
            | (screen << 12)
            | (u32::from(self.hudless) << 16)
            | (u32::from(self.timing_valid) << 17)
    }
}

fn provider_status(provider: &Provider, session: Handle, status: &mut ProviderStatus) -> i32 {
    status.struct_size = size_of::<ProviderStatus>() as u32;
    provider
        .status
        .map_or(FEATURE_FAILED, |f| unsafe { f(session, status) })
}

fn provider_configure(provider: &Provider, session: Handle, options: &rsf_fg_options) -> i32 {
    provider
        .configure
        .map_or(FEATURE_FAILED, |f| unsafe { f(session, options) })
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_prepare(
    _user: Handle,
    chain: Handle,
    _context: Handle,
    list: Handle,
    buffer: Handle,
    _host: Handle,
    session: Handle,
    sync: u32,
) {
    let p = &NATIVE_PLATFORM;
    TAGGED.set(None);
    let mut window: Pass = empty();
    window.struct_size = PASS_SIZE;
    let mut final_scene: SceneIdentity = empty();
    final_scene.struct_size = size_of::<SceneIdentity>() as u32;
    let options = requested();
    unsafe { crate::frame_limit::observe_pacing_display(options.frame_limit_us, chain) };
    let provider = unsafe { call!(p, present_provider).as_ref() };
    let Some(provider) = provider.filter(|_| !session.is_null()) else {
        INPUT_TIMING.set(InputTiming::default());
        let mut settings = lock(&SETTINGS);
        settings.cached.vendor = empty();
        settings.cached.last_result = 0;
        return;
    };
    let mut status: ProviderStatus = empty();
    provider_status(provider, session, &mut status);
    let window_matched = call!(p, window_match, chain, &mut window) != 0;
    let scene_matched =
        window_matched && call!(p, scene_for_window, &window, &mut final_scene) != 0;
    let presentation_path = if scene_matched {
        call!(p, scene_present_path, &window, &final_scene)
    } else {
        RSF_NATIVE_SCENE_PRESENT_NONE
    };
    let source = window.source_frame_id;
    let index = slot_index(source);
    // Contention means the render thread is filling this slot: refuse rather than tag it.
    let mut selected = if window_matched {
        try_slot(source)
    } else {
        None
    };
    let mut cpu: CpuFrame = empty();
    cpu.struct_size = size_of::<CpuFrame>() as u32;
    let mut owner = CpuOwner::default();
    let backend = call!(p, present_backend);
    let no_pacer = status.pacing_owner == RSF_PACING_NONE
        && options.reflex_mode == RSF_REFLEX_OFF
        && options.frame_limit_us == 0
        && (backend == RSF_NFG_FG_BACKEND_FSR3 || backend == RSF_NFG_FG_BACKEND_FSR4);
    let rendered = rendered_source();
    let generation = call!(p, present_generation);
    let mut submitted = false;
    if window_matched {
        owner = cpu_owner::frame_read(source);
        submitted = owner.id == source
            && owner.session == window.session_id
            && owner.submitted
            && owner.render_complete
            && (rendered || owner.paced || (owner.prospective && no_pacer))
            && !owner.failed
            && owner.provider_generation == generation;
    }
    let held = selected.as_deref();
    let inputs = held.is_some_and(|s| s.source == source && s.prepared);
    let hudless = inputs && held.is_some_and(|s| s.hudless_ready);
    let identity = scene_matched
        && held.is_some_and(|s| {
            s.pass.session_id == final_scene.session_id
                && s.pass.submission_id == final_scene.submission_id
                && s.pass.view_key == final_scene.view_key
                && s.pass.native_frame == final_scene.native_frame
        });
    let screen = held.map_or(0, |s| s.pass.screen);
    let pass_flags = held.map_or(0, |s| s.pass.flags);
    let native = held.map_or(0, |s| s.pass.native_frame);
    let scene_supported = inputs && generates_on(p, screen);
    let continuous = inputs
        && pass_flags & (RSF_GAME_RENDER_RESET | RSF_GAME_RENDER_NO_FG) == 0
        && window.flags & RSF_GAME_RENDER_NO_FG == 0;
    let vsync = sync == 0 || status.vsync_supported != 0;
    let rendered_valid = rendered
        && no_pacer
        && window_matched
        && unsafe { rsf_native_cpu_read(window.session_id, source, &mut cpu) } != 0
        && cpu.failed == 0
        && cpu.stage_mask & 9 == 9
        && owner.session == window.session_id
        && owner.id == source
        && !owner.failed
        && owner.rhi_begins == 1
        && owner.source_qpc != 0
        && owner.source_frequency != 0
        && owner.scene_begins == 1
        && owner.scene_ends == 1
        && owner.first_submission == window.submission_id
        && owner.first_view == window.view_key;
    let mut bound: Reservation = empty();
    bound.struct_size = size_of::<Reservation>() as u32;
    let reserved_bound = !owner.prospective
        || (unsafe { rsf_native_cpu_reservation_read(window.session_id, source, &mut bound) } != 0
            && (bound.state == RSF_NATIVE_CPU_RESERVATION_BOUND
                || bound.state == RSF_NATIVE_CPU_RESERVATION_CLOSED)
            && bound.poll_begin_qpc != 0
            && bound.poll_begin_qpc >= owner.ready_qpc);
    let measured_valid = !rendered
        && window_matched
        && unsafe { rsf_native_cpu_read(window.session_id, source, &mut cpu) } != 0
        && cpu.failed == 0
        && cpu.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE == 0
        && cpu.stage_mask & 7 == 7
        && owner.id == source
        && (owner.paced || (owner.prospective && no_pacer))
        && !owner.failed
        && reserved_bound;
    let input_qpc = cpu
        .input_ready_qpc
        .max(cpu.timestamps_qpc[RSF_GAME_CPU_INPUT_SAMPLE as usize]);
    // A discontinuity clears the predecessor. Only verified input from the same presentation
    // timeline can replace a missing engine delta.
    let sample = (measured_valid
        && submitted
        && identity
        && pass_flags & (RSF_GAME_RENDER_RESET | RSF_GAME_RENDER_NO_FG) == 0
        && window.flags & (RSF_GAME_RENDER_RESET | RSF_GAME_RENDER_NO_FG) == 0
        && input_qpc != 0
        && cpu.qpc_frequency != 0)
        .then_some(InputSample {
            timeline: SourceTimeline {
                session: window.session_id,
                generation,
                viewport: window.viewport_key,
                view: window.view_key,
                chain: chain as usize,
            },
            source,
            qpc: input_qpc,
            frequency: cpu.qpc_frequency,
        });
    let input_delta = INPUT_TIMING.with(|history| {
        let mut timing = history.get();
        let delta = timing.observe(sample);
        history.set(timing);
        delta
    });
    let measured_delta = if rendered {
        positive_delta(owner.source_delta_ms)
    } else {
        input_delta
    };
    let frame_time_ms = held
        .and_then(|slot| positive_delta(slot.pass.camera.frame_time_seconds * 1000.0))
        .or(measured_delta);
    let admission = Admission {
        window_matched,
        scene_matched,
        presentation_path,
        matched: presentation_path != RSF_NATIVE_SCENE_PRESENT_NONE,
        inputs,
        hudless,
        submitted,
        identity,
        scene_supported,
        continuous,
        vsync,
        cpu_valid: if rendered {
            rendered_valid
        } else {
            measured_valid
        },
        timing_valid: frame_time_ms.is_some(),
        rendered,
        no_pacer,
        reserved_bound,
    };
    // Slate/RHI may present while the game thread is finishing its frame. The renderer's copied
    // post-update identity authorizes these inputs; full SimulationEnd can arrive later.
    let valid = admission.valid();
    if let Some(held) = held
        && !rendered
        && options.debug != 0
        && inputs
        && admission.cpu_valid
        && owner.id == source
    {
        cpu_owner::report_timing(
            cpu_platform(p),
            &cpu,
            &owner,
            held.pass.screen,
            held.wait_ticks,
            held.evaluate_ticks,
        );
    }
    let mut configured = provider_options(&options, &status, valid);
    // Options are owner-thread calls. The provider handles transition-only SDK option updates.
    let row = format!(
        "prepare-configure selected_source={} window_native={} screen={} pass_flags={} window_flags={} inputs={} pass_reset={} pass_no_fg={} window_no_fg={} continuous={} \
         window_match={} rendered={} no_pacer={} submission_ok={} reserved_bound={} cpu_valid={} matched={} hudless={} identity={} scene_supported={} vsync={} valid={}",
        held.map_or(0, |s| s.pass.source_frame_id),
        window.native_frame,
        screen,
        pass_flags,
        window.flags,
        u32::from(inputs),
        u32::from(pass_flags & RSF_GAME_RENDER_RESET != 0),
        u32::from(pass_flags & RSF_GAME_RENDER_NO_FG != 0),
        u32::from(window.flags & RSF_GAME_RENDER_NO_FG != 0),
        u32::from(continuous),
        u32::from(window_matched),
        u32::from(admission.rendered),
        u32::from(admission.no_pacer),
        u32::from(submitted),
        u32::from(admission.reserved_bound),
        u32::from(admission.cpu_valid),
        u32::from(admission.matched),
        u32::from(hudless),
        u32::from(identity),
        u32::from(scene_supported),
        u32::from(vsync),
        u32::from(valid)
    );
    let result = if valid { OK } else { NOT_READY };
    cpu_owner::stage(
        cpu_platform(p),
        generation,
        &row,
        source,
        native,
        result,
        Some((&owner, &window)),
    );
    let configuration = provider_configure(provider, session, &configured);
    stage(p, c"prepare-configured", source, native, configuration);
    let eligibility = admission.eligibility(screen);
    let pending_generation = call!(p, present_generation);
    let diagnostic = {
        let mut settings = lock(&SETTINGS);
        settings.pending_generation = pending_generation;
        settings.cached.vendor = status;
        settings.cached.last_result = configuration;
        settings.cached.source_frame_id = source;
        if rendered {
            let same = owner.id == source;
            settings.cached.input_qpc = 0;
            settings.cached.source_qpc = if same { owner.source_qpc } else { 0 };
            settings.cached.source_qpc_frequency = if same { owner.source_frequency } else { 0 };
        }
        settings.cached.reason = admission.reason();
        let report = settings.requested.debug != 0
            && eligibility != settings.last_eligibility
            && settings.eligibility_lines < 256;
        if settings.requested.debug != 0 && eligibility != settings.last_eligibility {
            settings.eligibility_lines = settings.eligibility_lines.saturating_add(1);
        }
        if report {
            settings.last_eligibility = eligibility;
            Sink(settings.log, settings.log_user)
        } else {
            Sink(None, ptr::null_mut())
        }
    };
    if diagnostic.0.is_some() {
        diagnostic.say(&format!(
            "FG eligibility source={} window={} scene={} sampled={} direct={} inputs={} hudless={} submission={} identity={} allowed_scene={} continuous={} vsync={} cpu={} mask={} screen={} flags={} configure={}",
            source,
            u32::from(window_matched),
            u32::from(scene_matched),
            u32::from(presentation_path == RSF_NATIVE_SCENE_PRESENT_SAMPLED),
            u32::from(presentation_path == RSF_NATIVE_SCENE_PRESENT_DIRECT),
            u32::from(inputs),
            u32::from(hudless),
            u32::from(submitted),
            u32::from(identity),
            u32::from(scene_supported),
            u32::from(continuous),
            u32::from(vsync),
            u32::from(admission.cpu_valid),
            cpu.stage_mask,
            screen,
            pass_flags,
            configuration
        ));
    }
    if configuration != OK || !valid {
        return;
    }
    let Some(selected) = selected.as_deref_mut() else {
        return;
    };
    let mut record: rsf_frame_record = empty();
    record.struct_size = size_of::<rsf_frame_record>() as u32;
    record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.session_id = window.session_id;
    record.frame_id = source;
    record.resource_generation = selected.generation;
    record.view_id = selected.pass.view_key as u32;
    record.input_qpc = if rendered { 0 } else { input_qpc };
    if rendered {
        record.source_provenance = RSF_FRAME_SOURCE_RENDERED;
        record.source_qpc = owner.source_qpc;
        record.source_qpc_frequency = owner.source_frequency;
    }
    record.phase = RSF_NFG_PHASE_UI_COMPLETE;
    record.screen = selected.pass.screen;
    record.render_width = selected.width;
    record.render_height = selected.height;
    record.output_width = selected.output_width;
    record.output_height = selected.output_height;
    record.camera = selected.pass.camera;
    let Some(frame_time_ms) = frame_time_ms else {
        return;
    };
    record.frame_time_ms = frame_time_ms;

    {
        let mut settings = lock(&SETTINGS);
        settings.cached.input_qpc = record.input_qpc;
        settings.cached.source_qpc = record.source_qpc;
        settings.cached.source_qpc_frequency = record.source_qpc_frequency;
    }
    record.camera.struct_size = size_of_val(&record.camera) as u32;
    record.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    let mut frame: rsf_fg_frame = empty();
    frame.struct_size = size_of::<rsf_fg_frame>() as u32;
    frame.record = &record;
    frame.interpolate = u32::from(options.mode != RSF_FG_OFF);
    frame.motion_scale_x = 1.0;
    frame.motion_scale_y = 1.0;
    // Every provider gets its inputs in the read state. Retirement restores them for D3D11 reuse
    // once the vendor's reads, which can run until interpolation at Present, have finished.
    let native_resources = [
        buffer,
        d3d12(&selected.surfaces[DEPTH]),
        d3d12(&selected.surfaces[MOTION]),
        d3d12(&selected.hudless),
    ];
    let display = (selected.output_width, selected.output_height);
    let render = (selected.width, selected.height);
    let targets: [&mut rsf_backend_resource; 4] = [
        &mut frame.backbuffer,
        &mut frame.depth,
        &mut frame.motion,
        &mut frame.hudless,
    ];
    for (i, target) in targets.into_iter().enumerate() {
        let (width, height) = if i == 0 || i == 3 { display } else { render };
        target.struct_size = size_of::<rsf_backend_resource>() as u32;
        target.resource = native_resources[i];
        target.generation = selected.generation;
        target.width = width;
        target.height = height;
        target.state = if i == 0 { RSF_NFG_STATE_PRESENT } else { READ };
        if i != 0 {
            call!(p, transition, list, native_resources[i], COMMON, READ);
        }
    }
    selected.restore = true;
    let native = selected.pass.native_frame;
    stage(p, c"provider-prepare-enter", source, native, 0);
    let preparation = provider
        .prepare
        .map_or(FEATURE_FAILED, |f| unsafe { f(session, list, &frame) });
    stage(p, c"provider-prepare-done", source, native, preparation);
    call!(p, dump_fg_prepare, &selected.pass, &frame, preparation);
    selected.prepared = preparation == OK;
    {
        let mut settings = lock(&SETTINGS);
        settings.cached.last_result = preparation;
        if selected.prepared && frame.interpolate != 0 {
            settings.cached.tagged_frames += 1;
        }
    }
    trace(c"prepare/tags", source, preparation);
    TAGGED.set(Some(index));
    if !selected.prepared {
        configured.mode = RSF_FG_OFF;
        provider_configure(provider, session, &configured);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_retire(_user: Handle, session: Handle) {
    let p = &NATIVE_PLATFORM;
    let provider = unsafe { call!(p, present_provider).as_ref() };
    let Some(provider) = provider.filter(|_| !session.is_null()) else {
        // No provider is left to read the inputs, but they are still in the read state.
        if let Some(index) = TAGGED.take() {
            let mut slot = lock(&SLOTS[index]);
            if slot.restore {
                lock(&VENDOR)[index].read_state = true;
                slot.restore = false;
            }
        }
        let mut settings = lock(&SETTINGS);
        settings.cached.vendor = empty();
        settings.completed_generation = 0;
        settings.completed = empty();
        consume_completed_present();
        return;
    };
    let mut state: ProviderStatus = empty();
    let status_result = provider_status(provider, session, &mut state);
    let generation = call!(p, present_generation);
    let (completed_source, completed_generation, completed_ok) = consume_completed_present();
    if status_result == OK {
        let (changed, log) = {
            let mut settings = lock(&SETTINGS);
            let changed = settings.cached.vendor.active != state.active
                || settings.cached.vendor.effective_mode != state.effective_mode;
            settings.cached.vendor = state;
            (settings.completed_generation, settings.completed) = fg_status::observe_completed(
                &settings.cached,
                settings.pending_generation,
                completed_generation,
                completed_source,
                completed_ok,
                generation,
            );
            (changed, Sink(settings.log, settings.log_user))
        };
        if changed {
            log.say(&format!(
                "FG effective mode={} generated={} active={} total presents={} status={}",
                state.effective_mode,
                state.effective_generated_frames,
                state.active,
                state.total_presented,
                state.vendor_status
            ));
        }
    } else {
        let mut settings = lock(&SETTINGS);
        settings.completed_generation = 0;
        settings.completed = empty();
        settings.cached.vendor.active = 0;
        settings.cached.last_result = status_result;
    }
    let index = TAGGED.take();
    let held = lock(&VENDOR).iter().any(|vendor| vendor.unknown);
    // A frame without tags (FG off, an ineligible screen) still joins while slots are held, so SR
    // evaluation never waits on a retirement that only a generated frame would bring.
    if index.is_none() && !held {
        return;
    }
    let mut slot = index.map(|index| lock(&SLOTS[index]));
    // The read-state restore runs on the provider queue, after the vendor's reads.
    let restoring = slot.as_ref().is_some_and(|s| s.restore);
    let mut restore = ptr::null_mut();
    if restoring
        && let Some(slot) = slot.as_deref()
        && let Some(retirement) = slot.retirement.as_ref().filter(|c| reset(p, c))
    {
        let list = retirement.list.as_ptr();
        for surface in [&slot.surfaces[DEPTH], &slot.surfaces[MOTION], &slot.hudless] {
            if surface.is_some() {
                call!(p, transition, list, d3d12(surface), READ, COMMON);
            }
        }
        if call!(p, close_commands, list) != 0 {
            restore = list;
        }
    }
    let (mut fence, mut serial) = (ptr::null_mut(), 0u64);
    let result = if restoring && restore.is_null() {
        FEATURE_FAILED
    } else {
        call!(p, present_retire, session, restore, &mut fence, &mut serial)
    };
    if let Some(slot) = slot.as_deref_mut() {
        slot.restore = false;
    }
    let generation = call!(p, present_generation);
    {
        let mut vendor = lock(&VENDOR);
        if result == OK && !fence.is_null() {
            let joined = unsafe { ComRef::retain(fence) };
            if let Some(index) = index {
                let own = &mut vendor[index];
                own.fence = Some(joined.clone());
                own.value = serial;
                own.unknown = false;
                own.read_state = false;
            }
            // This join also covers provider-queue reads of earlier frames, so it releases slots
            // whose own retirement failed. Their read_state stays for resources() to restore.
            for other in vendor.iter_mut().filter(|other| other.unknown) {
                other.fence = Some(joined.clone());
                other.value = serial;
                other.unknown = false;
            }
            return;
        }
        let Some(index) = index else {
            return;
        };
        // Nothing was signalled. Hold the slot rather than wait on a value that never arrives. A
        // failure before the bridge ran the list leaves the surfaces in the read state.
        let own = &mut vendor[index];
        own.fence = None;
        own.value = 0;
        own.unknown = true;
        own.generation = generation;
        own.read_state = restoring;
    }
    let source = slot.as_ref().map_or(0, |s| s.source);
    trace(
        c"retirement failed; slot held until a later retirement",
        source,
        result,
    );
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_native_fg_present(event: *const PresentEvent) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    let p = &NATIVE_PLATFORM;
    if event.flags & RSF_NFG_PRESENT_TEST != 0 || call!(p, present_has_owner) == 0 {
        return;
    }
    if event.completed == 0 {
        let mut window: Pass = empty();
        window.struct_size = PASS_SIZE;
        let presenting = if call!(p, window_match, event.swapchain, &mut window) != 0 {
            window.source_frame_id
        } else {
            0
        };
        let session = if presenting != 0 {
            window.session_id
        } else {
            0
        };
        PRESENTING.set((presenting, session));
        cpu_owner::present_observed(presenting, session);
        cpu_owner::latency_present(cpu_platform(p), presenting, event.swapchain, true);
    } else {
        let (presenting, session) = PRESENTING.get();
        let generation = call!(p, present_generation);
        COMPLETED_PRESENT.set((presenting, generation, event.result >= 0));
        if presenting != 0 {
            rsf_native_cpu_present_result(
                session,
                presenting,
                RSF_NATIVE_CPU_PRESENT_RETURNED,
                event.result,
            );
        }
        cpu_owner::latency_present(cpu_platform(p), presenting, event.swapchain, false);
        PRESENTING.set((0, 0));
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_window_end(pass: *const Pass) {
    let p = &NATIVE_PLATFORM;
    if pass.is_null() || unsafe { pass.cast::<u32>().read() } < PASS_SIZE {
        return;
    }
    let pass = unsafe { &*pass };
    let source = pass.source_frame_id;
    let owner = call!(p, present_has_owner) != 0;
    if source == 0 || (owner && call!(p, present_is_owner, pass.swapchain) == 0) {
        return;
    }
    cpu_owner::count_window(source);
    if !owner {
        return;
    }
    let (live, presented) = cpu_owner::progress(cpu_platform(p), source);
    // The window ended without presenting a live source: its token cannot carry a later frame.
    if live && !presented {
        call!(p, present_abort, source);
        cpu_owner::fail(source);
        cpu_owner::complete(cpu_platform(p), source);
    }
}

#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_native_fg_inputs_ready(source: u64) -> i32 {
    if source == 0 {
        return 0;
    }
    let slot = slot(source);
    i32::from(
        slot.prepared
            && slot.source == source
            && slot.surfaces[DEPTH].is_some()
            && slot.surfaces[MOTION].is_some(),
    )
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_native_fg_dump_inputs(
    context: Handle,
    source: u64,
    directory: *const c_char,
    prefix: *const c_char,
) -> i32 {
    let p = &NATIVE_PLATFORM;
    if context.is_null()
        || source == 0
        || directory.is_null()
        || prefix.is_null()
        || unsafe { directory.read() } == 0
        || unsafe { prefix.read() } == 0
    {
        return 0;
    }
    let prefix = unsafe { CStr::from_ptr(prefix) }.to_bytes();
    let log = sink(false);
    let slot = slot(source);
    if slot.source != source || !slot.prepared {
        return 0;
    }
    let hudless = if slot.hudless_ready {
        d3d11(&slot.hudless)
    } else {
        ptr::null_mut()
    };
    let items = [
        (
            d3d11(&slot.surfaces[DEPTH]),
            "_fg_depth",
            RSF_NFG_DUMP_VIEW_DEPTH,
            1.0,
        ),
        (
            d3d11(&slot.surfaces[MOTION]),
            "_fg_motion",
            RSF_NFG_DUMP_VIEW_DENSE_MOTION,
            8.0,
        ),
        (hudless, "_fg_hudless", RSF_NFG_DUMP_VIEW_RAW, 1.0),
    ];
    let mut written = 0;
    for (texture, suffix, view, scale) in items {
        if texture.is_null() {
            continue;
        }
        // Names keep the 159-byte bound of the dump service's existing callers.
        let mut name = [prefix, suffix.as_bytes()].concat();
        name.truncate(159);
        let Ok(name) = CString::new(name) else {
            continue;
        };
        let result = call!(
            p,
            dump_named,
            context,
            texture,
            directory,
            name.as_ptr(),
            view,
            scale,
            log.0,
            log.1
        );
        if result == 0 {
            written += 1;
        }
    }
    written
}

pub(super) fn provider_options(
    options: &Options,
    status: &ProviderStatus,
    valid: bool,
) -> rsf_fg_options {
    // The requested limit is the interval of rendered frames, before generation. With generation
    // active the driver's Reflex limiter counts every presented frame (measured: 10000 us requested
    // at 2x paced rendered frames 19.9 ms apart), so it is given the interval between presents. This
    // follows the frame's eligibility, not the SDK's activity report: waiting for that report left
    // the undivided interval in place for the first generated frame, which the driver doubled into
    // one 25 ms frame at every activation. The cost of deciding early is a looser limit on eligible
    // frames the SDK is not yet generating for.
    let generated = options.generated_frames.min(status.max_generated_frames);
    let presents_per_frame =
        if status.pacing_owner == RSF_PACING_REFLEX && valid && options.mode == RSF_FG_FIXED {
            generated.saturating_add(1)
        } else {
            1
        };
    let paced = status.pacing_owner != RSF_PACING_NONE;
    let mut effective = *status;
    if !valid {
        crate::fg_status::deactivate(&mut effective);
    }
    rsf_fg_options {
        struct_size: size_of::<rsf_fg_options>() as u32,
        abi_version: RSF_FG_ABI_VERSION,
        mode: if valid { options.mode } else { RSF_FG_OFF },
        generated_frames: generated,
        dynamic_target_fps: 0.0,
        reflex_mode: if paced {
            options.reflex_mode
        } else {
            RSF_REFLEX_OFF
        },
        frame_limit_us: match status.pacing_owner {
            RSF_PACING_REFLEX => {
                crate::frame_limit::reflex_interval(options.frame_limit_us, presents_per_frame)
            }
            RSF_PACING_NONE => 0,
            _ => crate::frame_limit::application_interval(options.frame_limit_us, &effective),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admission() -> Admission {
        Admission {
            window_matched: true,
            scene_matched: true,
            presentation_path: RSF_NATIVE_SCENE_PRESENT_DIRECT,
            matched: true,
            inputs: true,
            hudless: true,
            submitted: true,
            identity: true,
            scene_supported: true,
            continuous: true,
            vsync: true,
            cpu_valid: true,
            timing_valid: true,
            rendered: false,
            no_pacer: false,
            reserved_bound: true,
        }
    }

    #[test]
    fn reasons_follow_the_first_missing_fact() {
        let mut facts = admission();
        assert!(facts.valid());
        assert_eq!(facts.reason(), 0);
        facts.hudless = false;
        assert_eq!(facts.reason(), 8);
        facts.vsync = false;
        assert_eq!(facts.reason(), 6);
        facts.continuous = false;
        assert_eq!(facts.reason(), 5);
        facts.scene_supported = false;
        assert_eq!(facts.reason(), 4);
        facts.submitted = false;
        assert_eq!(facts.reason(), 3);
        facts.inputs = false;
        assert_eq!(facts.reason(), 2);
        facts.matched = false;
        assert_eq!(facts.reason(), 1);
        let mut identity = admission();
        identity.identity = false;
        assert_eq!(identity.reason(), 7);
        identity.identity = true;
        identity.cpu_valid = false;
        assert_eq!(identity.reason(), 7);
    }

    #[test]
    fn eligibility_mask_keeps_its_bit_layout() {
        let facts = admission();
        assert_eq!(
            facts.eligibility(3),
            0x3ff | (2 << 10) | (3 << 12) | (1 << 16) | (1 << 17)
        );
    }

    #[test]
    fn measured_input_time_requires_a_consecutive_matching_timeline() {
        let previous = InputSample {
            timeline: SourceTimeline {
                session: 1,
                generation: 2,
                viewport: 3,
                view: 4,
                chain: 5,
            },
            source: 6,
            qpc: 1000,
            frequency: 1000,
        };
        let mut current = InputSample {
            source: 7,
            qpc: 1020,
            ..previous
        };
        assert_eq!(current.delta_from(previous), Some(20.0));
        let mut timing = InputTiming::default();
        assert_eq!(timing.observe(Some(previous)), None);
        assert_eq!(timing.observe(Some(current)), Some(20.0));
        assert_eq!(timing.observe(Some(current)), Some(20.0));
        assert_eq!(timing.observe(None), None);
        assert_eq!(timing.observe(Some(current)), None);
        current.timeline.session += 1;
        assert_eq!(current.delta_from(previous), None);
        current.timeline = previous.timeline;
        current.timeline.generation += 1;
        assert_eq!(current.delta_from(previous), None);
        current.timeline = previous.timeline;
        current.source += 1;
        assert_eq!(current.delta_from(previous), None);
        assert_eq!(positive_delta(0.0), None);
        assert_eq!(positive_delta(f32::NAN), None);
    }

    #[test]
    fn hudless_formats_are_the_shareable_display_formats() {
        for format in [10, 24, 28, 29, 87, 91] {
            assert!(hudless_format(format));
        }
        for format in [2, 16, 26, 34, 41] {
            assert!(!hudless_format(format));
        }
    }

    #[test]
    fn options_refuse_short_and_out_of_range_requests() {
        let before = requested();
        let mut options = Options {
            struct_size: size_of::<Options>() as u32 - 1,
            mode: RSF_FG_FIXED,
            generated_frames: 2,
            reflex_mode: RSF_REFLEX_OFF,
            frame_limit_us: 0,
            debug: 0,
        };
        unsafe { rsf_native_fg_options_set(&options) };
        assert_eq!(requested().mode, before.mode);
        options.struct_size = size_of::<Options>() as u32;
        options.mode = RSF_FG_DYNAMIC + 1;
        unsafe { rsf_native_fg_options_set(&options) };
        assert_eq!(requested().mode, before.mode);
        options.mode = RSF_FG_FIXED;
        options.reflex_mode = RSF_REFLEX_BOOST + 1;
        unsafe { rsf_native_fg_options_set(&options) };
        assert_eq!(requested().mode, before.mode);
    }

    #[test]
    fn slots_hold_only_their_exact_pass() {
        let mut slot = Slot::new();
        let mut pass: Pass = empty();
        pass.source_frame_id = 7;
        pass.session_id = 3;
        pass.view_key = 11;
        pass.submission_id = 13;
        pass.native_frame = 17;
        slot.source = 7;
        slot.pass = pass;
        assert!(slot.holds(&pass));
        for change in 0..5 {
            let mut other = pass;
            match change {
                0 => other.source_frame_id = 8,
                1 => other.session_id = 4,
                2 => other.view_key = 12,
                3 => other.submission_id = 14,
                _ => other.native_frame = 18,
            }
            assert!(!slot.holds(&other));
        }
        assert_eq!(slot_index(7), slot_index(13));
    }
}
