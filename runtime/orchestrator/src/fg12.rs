// SPDX-License-Identifier: MIT
use crate::com_ref::ComRef;
use crate::cpu_abi::rsf_native_cpu_frame;
use crate::fg_leases::{FencePoint, FenceState};
use crate::fg12_abi::*;
use crate::sr_session::{checked, has_capacity};

use crate::native_cpu::rsf_native_cpu_read;
use crate::overlay_abi::rsf_native_latency_status;
use crate::render_abi::{
    RSF_FG_BACKEND_ALL, RSF_FG_BACKEND_AUTO, RSF_FG_FIXED, RSF_FG_OFF, RSF_REFLEX_BOOST,
    RSF_REFLEX_OFF, rsf_backend_log_fn, rsf_backend_resource, rsf_fg_frame, rsf_fg_status,
    rsf_native_fg_options, rsf_native_fg_status,
};
use crate::scene_abi::rsf_observer_present_event;
use rsf_game_sys::{
    RSF_FRAME_FLAG_AMBIGUOUS_ID, RSF_FRAME_FLAG_NO_FG, RSF_FRAME_FLAG_NO_SR, RSF_FRAME_FLAG_RESET,
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END,
    RSF_GAME_CPU_INPUT_KEYED, RSF_GAME_CPU_INPUT_SAMPLE, RSF_GAME_CPU_INPUT_SUBSTITUTE,
    RSF_GAME_CPU_PACING, RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END,
    RSF_GAME_FRAME_ABI_VERSION, rsf_frame_record, rsf_game_cpu_event, rsf_game_latency_event,
    rsf_game_render_pass,
};
use std::cell::Cell;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use std::sync::OnceLock;

static PLATFORM: OnceLock<&'static rsf_fg12_calls> = OnceLock::new();

#[cfg(not(test))]
mod native;

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub(super) extern "C" fn rsf_fg12_platform_bind() {
    PLATFORM.get_or_init(|| &native::CALLS);
}

#[cfg(not(test))]
pub(super) use rsf_fg12_platform_bind as bind_native_platform;

macro_rules! native {
    ($operation:ident($($argument:expr),* $(,)?)) => {
        (PLATFORM.get().unwrap().$operation.unwrap())($($argument),*)
    };
}

mod colors;
mod retirement;
use colors::Colors;
use retirement::VendorUse;

const OK: i32 = 0;
const INVALID: i32 = -1;
const INIT_FAILED: i32 = -6;
const NOT_SUPPORTED: i32 = -7;
const NOT_READY: i32 = -8;
const NEEDS_RESTART: i32 = -11;

const COMMON: u32 = 0;
const PRESENT_STATE: u32 = 0;
const RENDER_TARGET: u32 = 4;
const SHADER_READ: u32 = 64;
const PRESENT_TEST: u32 = 1;
const UI_COMPLETE: u32 = 5;
const SIMULATION_START: u32 = 0;
const SIMULATION_END: u32 = 1;
const SUBMIT_START: u32 = 2;
const PRESENT_END: u32 = 5;
const INPUT_SAMPLE: u32 = 6;

const INPUT_SLOTS: usize = 6;
const CPU_SLOTS: usize = 128;
const BACK_BUFFERS: usize = 8;

const OPTIONS: rsf_native_fg_options = rsf_native_fg_options {
    struct_size: size_of::<rsf_native_fg_options>() as u32,
    mode: RSF_FG_OFF,
    generated_frames: 1,
    reflex_mode: RSF_REFLEX_OFF,
    frame_limit_us: 0,
    debug: 0,
};

fn empty_record() -> rsf_frame_record {
    rsf_frame_record {
        struct_size: size_of::<rsf_frame_record>() as u32,
        ..unsafe { std::mem::zeroed() }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Frame {
    session: u64,
    id: u64,
}

impl Frame {
    fn index(self, count: usize) -> usize {
        (self.id % count as u64) as usize
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct View {
    frame: Frame,
    key: u32,
}

impl View {
    fn pass(pass: &rsf_game_render_pass) -> Self {
        Self {
            frame: Frame {
                session: pass.session_id,
                id: pass.source_frame_id,
            },
            key: pass.view_key as u32,
        }
    }

    fn matches(self, record: &rsf_frame_record) -> bool {
        self.frame.id != 0
            && self.frame.id == record.frame_id
            && self.frame.session == record.session_id
            && self.key == record.view_id
    }
}

#[derive(Clone, Copy)]
struct Cpu {
    frame: Frame,
    input: u64,
    generation: u64,
    begun: bool,
    simulation: bool,
    failed: bool,
    render_started: bool,
}

impl Cpu {
    const EMPTY: Self = Self {
        frame: Frame { session: 0, id: 0 },
        input: 0,
        generation: 0,
        begun: false,
        simulation: false,
        failed: false,
        render_started: false,
    };
}

struct Texture {
    resource: ComRef,
    state: u32,
}

impl Texture {
    unsafe fn allocate(list: *mut c_void, source: *mut c_void, old: Option<&Self>) -> Option<Self> {
        let reuse = old.map_or(null_mut(), |texture| texture.resource.as_ptr());
        let raw = unsafe { native!(texture(list, source, reuse)) };
        let resource = unsafe { ComRef::from_owned(raw) }?;
        Some(Self {
            resource,
            state: if raw == reuse {
                old.map_or(COMMON, |texture| texture.state)
            } else {
                COMMON
            },
        })
    }

    unsafe fn copy_from(&mut self, list: *mut c_void, src: *mut c_void, state: u32) {
        unsafe { native!(copy(list, src, state, self.resource.as_ptr(), self.state)) };
        self.state = COMMON;
    }

    unsafe fn read(&mut self, list: *mut c_void) -> *mut c_void {
        let raw = self.resource.as_ptr();
        unsafe { native!(transition(list, raw, self.state, SHADER_READ)) };
        self.state = SHADER_READ;
        raw
    }
}

struct Slot {
    record: rsf_frame_record,
    depth: Texture,
    motion: Texture,
    hudless: Option<Texture>,
    source_queue: ComRef,
    source_fence: Option<FencePoint>,
    // A submit callback covers exactly the copies recorded for this frame so far.
    submitted: bool,
    pending_copy: bool,
    hudless_recorded: bool,
    vendor: VendorUse,
}

impl Slot {
    fn eligible(&self, view: View) -> bool {
        self.submitted
            && !self.pending_copy
            && matches!(self.vendor, VendorUse::None)
            && view.matches(&self.record)
    }

    fn reusable(&self, generation: u64) -> bool {
        if matches!(self.vendor, VendorUse::None) {
            !self.pending_copy
                && self
                    .source_fence
                    .as_ref()
                    .is_some_and(|point| point.state() == FenceState::Complete)
        } else {
            self.vendor.reusable(generation)
        }
    }
}

struct SourceFence {
    queue: ComRef,
    fence: ComRef,
    serial: u64,
}

impl SourceFence {
    unsafe fn signal(&mut self) -> Option<FencePoint> {
        let value = self
            .serial
            .checked_add(1)
            .filter(|&value| value < u64::MAX)?;
        if unsafe { native!(signal(self.queue.as_ptr(), self.fence.as_ptr(), value)) } == 0 {
            return None;
        }
        self.serial = value;
        Some(FencePoint {
            fence: self.fence.clone(),
            value,
        })
    }
}

#[derive(Clone, Copy, Default)]
struct Present {
    view: View,
    generation: u64,
    // Set only by a valid preparation callback which records access to the slot.
    prepared: bool,
}

thread_local! {
    static PRESENT: Cell<Present> = Cell::new(Present::default());
}

struct Owner {
    options: rsf_native_fg_options,
    ui_mode: u32,
    windows: [View; BACK_BUFFERS],
    cpu: [Cpu; CPU_SLOTS],
    slots: [Option<Slot>; INPUT_SLOTS],
    source_fence: Option<SourceFence>,
    colors: Colors,
    state: rsf_native_fg_status,
    generation: u64,
    samples: u64,
    diagnostics: u64,
    prepare_failures: u32,
}

// Only copied frame facts and retained free-threaded D3D12 interfaces enter this mutex.
// The lists and the physical swapchain buffers remain borrowed for their callbacks.
unsafe impl Send for Owner {}

static OWNER: Mutex<Owner> = Mutex::new(Owner {
    options: OPTIONS,
    ui_mode: RSF_UI_MODE_NONE,
    windows: [View {
        frame: Frame { session: 0, id: 0 },
        key: 0,
    }; BACK_BUFFERS],
    cpu: [Cpu::EMPTY; CPU_SLOTS],
    slots: [const { None }; INPUT_SLOTS],
    source_fence: None,
    colors: Colors::new(),
    state: unsafe { std::mem::zeroed() },
    generation: 0,
    samples: 0,
    diagnostics: 0,
    prepare_failures: 0,
});

static INSTALL: AtomicU8 = AtomicU8::new(0); // 0 idle, 1 installing, 2 installed
static LOGGER: Mutex<(rsf_backend_log_fn, usize)> = Mutex::new((None, 0));

thread_local! {
    static INSTALL_LOGGER: Cell<(rsf_backend_log_fn, usize)> = const { Cell::new((None, 0)) };
}

unsafe extern "C" fn log_callback(_: *mut c_void, message: *const c_char) {
    let (callback, user) = if INSTALL.load(Ordering::Acquire) == 1 {
        INSTALL_LOGGER.get()
    } else {
        *LOGGER.lock().unwrap()
    };
    if let Some(callback) = callback {
        unsafe { callback(user as *mut c_void, message) };
    }
}

type WindowFilter = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32;
static WINDOW_FILTER: OnceLock<(WindowFilter, usize)> = OnceLock::new();

unsafe extern "C" fn accept_window(_: *mut c_void, window: *mut c_void) -> i32 {
    if INSTALL.load(Ordering::Acquire) != 2 {
        return 0;
    }
    let (filter, user) = *WINDOW_FILTER.get().unwrap();
    unsafe { filter(user as *mut c_void, window) }
}

fn log(message: String) {
    if let Ok(message) = CString::new(message) {
        unsafe { log_callback(null_mut(), message.as_ptr()) };
    }
}

fn options_valid(options: &rsf_native_fg_options) -> bool {
    options.struct_size >= OPTIONS.struct_size
        && options.mode <= RSF_FG_FIXED
        && options.reflex_mode <= RSF_REFLEX_BOOST
        && options.generated_frames != 0
}

unsafe fn install(request: *const rsf_fg12_setup, explicit: bool) -> i32 {
    let Some(request) = (unsafe { checked(request) }) else {
        return INVALID;
    };
    let Some(window_filter) = request.accept_window else {
        return INVALID;
    };
    if request.runtime_directory_utf8.is_null()
        || !options_valid(&request.options)
        || request.units_to_meters <= 0.0
    {
        return INVALID;
    }
    if INSTALL.load(Ordering::Acquire) != 0 {
        return NEEDS_RESTART;
    }
    let can_switch = request.runtime_switching != 0;
    let known = 1_u32
        .checked_shl(request.backend)
        .is_some_and(|bit| bit & RSF_FG_BACKEND_ALL != 0);
    if !(known || can_switch && matches!(request.backend, 0 | RSF_FG_BACKEND_AUTO)) {
        return NOT_SUPPORTED;
    }
    if INSTALL
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return NEEDS_RESTART;
    }
    let ui_mode = if request.ui_mode == 0 {
        RSF_UI_MODE_NONE
    } else {
        request.ui_mode
    };
    let setup = rsf_d3d11_present_setup {
        struct_size: size_of::<rsf_d3d11_present_setup>() as u32,
        runtime_directory_utf8: request.runtime_directory_utf8,
        backend: request.backend,
        max_generated_frames: request.max_generated_frames,
        depth_inverted: request.depth_inverted,
        depth_infinite: request.depth_infinite,
        units_to_meters: request.units_to_meters,
        runtime_switching: request.runtime_switching,
        ui_mode,
        fsr3_directory_utf8: request.fsr3_directory_utf8,
        fsr4_directory_utf8: request.fsr4_directory_utf8,
        xess_directory_utf8: request.xess_directory_utf8,
        streamline_directory_utf8: request.streamline_directory_utf8,
        engine_type: request.engine_type,
        engine_version_utf8: request.engine_version_utf8,
        project_id_utf8: request.project_id_utf8,
        log: Some(log_callback),
        user: null_mut(),
        accept_window: Some(accept_window),
        prepare: Some(prepare),
        retire: Some(retire),
        latency_event: Some(latency),
        ..unsafe { std::mem::zeroed() }
    };
    // The native bridge only retains the process-lifetime callbacks above.
    // Its synchronous install receipt can use the caller's still-borrowed logger.
    INSTALL_LOGGER.set((request.log, request.user as usize));
    let installed = unsafe { native!(install(&setup, i32::from(explicit))) };
    INSTALL_LOGGER.set((None, 0));
    if installed == 0 {
        INSTALL.store(0, Ordering::Release);
        return INIT_FAILED;
    }
    {
        let mut owner = OWNER.lock().unwrap();
        owner.options = request.options;
        owner.ui_mode = ui_mode;
    }
    *LOGGER.lock().unwrap() = (request.log, request.user as usize);
    unsafe {
        native!(latency_graphics(12));
        native!(fg_options_set(&request.options));
        native!(fg_set_log(request.log, request.user));
    }
    let _ = WINDOW_FILTER.set((window_filter, request.user as usize));
    INSTALL.store(2, Ordering::Release);
    OK
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_install(request: *const rsf_fg12_setup) -> i32 {
    unsafe { install(request, false) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_install_explicit(
    request: *const rsf_fg12_setup,
) -> i32 {
    unsafe { install(request, true) }
}

fn latency_frame(id: u64) -> Option<rsf_native_latency_frame> {
    let mut core = rsf_native_latency_frame {
        struct_size: size_of::<rsf_native_latency_frame>() as u32,
        version: 1,
        source: id,
        ..unsafe { std::mem::zeroed() }
    };
    (unsafe { native!(latency_frame_get(&mut core)) } != 0).then_some(core)
}

unsafe extern "C" fn cpu_fact(_: *mut c_void, event: *const rsf_game_cpu_event) {
    // The shared owner can sleep here. Present and render must still be able to
    // progress older frames while the provider waits for them.
    unsafe { native!(fg_cpu(event)) };
    let event = unsafe { &*event };
    let Some(core) = latency_frame(event.source_frame_id) else {
        return;
    };
    let frame = Frame {
        session: event.session_id,
        id: event.source_frame_id,
    };
    let mut owner = OWNER.lock().unwrap();
    let timing = &mut owner.cpu[frame.index(CPU_SLOTS)];
    if event.stage == RSF_GAME_CPU_FRAME_BEGIN {
        *timing = Cpu {
            frame,
            ..Cpu::EMPTY
        };
    }
    if timing.frame != frame {
        return;
    }
    timing.generation = core.generation;
    timing.failed |= core.failed != 0;
    timing.begun = core.paced != 0;
    if event.stage == RSF_GAME_CPU_INPUT_SAMPLE {
        timing.input = event.timestamp_qpc.max(core.ready_qpc);
        timing.failed |= event.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE != 0;
    }
    if event.stage == RSF_GAME_CPU_SIMULATION_END {
        timing.simulation = true;
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_cpu(event: *const rsf_game_cpu_event) {
    if unsafe { native!(has_owner()) } != 0 {
        unsafe { native!(cpu_dispatch(event, Some(cpu_fact), null_mut())) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_latency(event: *const rsf_game_latency_event) -> i32 {
    let Some(event) = (unsafe { checked(event) }).filter(|event| event.session_id != 0) else {
        return INVALID;
    };
    if INSTALL.load(Ordering::Acquire) != 2 {
        return NOT_READY;
    }
    if event.marker == RSF_GAME_LATENCY_MODE {
        return OK;
    }
    if event.source_frame_id == 0 || event.timestamp_qpc == 0 || event.qpc_frequency == 0 {
        return INVALID;
    }
    let mut fact = rsf_game_cpu_event {
        struct_size: size_of::<rsf_game_cpu_event>() as u32,
        stage: RSF_GAME_CPU_FRAME_BEGIN,
        session_id: event.session_id,
        source_frame_id: event.source_frame_id,
        timestamp_qpc: event.timestamp_qpc,
        qpc_frequency: event.qpc_frequency,
        render_expected: 1,
        event_version: RSF_GAME_CPU_EVENT_VERSION,
        flags: RSF_GAME_CPU_INPUT_KEYED,
        ..unsafe { std::mem::zeroed() }
    };
    match event.marker {
        RSF_GAME_LATENCY_SLEEP => {
            unsafe { rsf_core_fg12_cpu(&fact) };
            // The engine's actual pre-input wait is the pacing deadline.
            fact.stage = RSF_GAME_CPU_PACING;
            fact.event_version = 0;
            fact.flags = 0;
        }
        INPUT_SAMPLE => fact.stage = RSF_GAME_CPU_INPUT_SAMPLE,
        SIMULATION_START => fact.stage = RSF_GAME_CPU_SIMULATION_BEGIN,
        SIMULATION_END => fact.stage = RSF_GAME_CPU_SIMULATION_END,
        SUBMIT_START => return rsf_core_fg12_render_begin(event.session_id, event.source_frame_id),
        PRESENT_END => {
            fact.stage = RSF_GAME_CPU_FRAME_END;
            unsafe { rsf_core_fg12_cpu(&fact) };
            return OK;
        }
        // Render-end and Present markers come from the actual presentation path.
        marker if marker <= INPUT_SAMPLE => return OK,
        _ => return INVALID,
    }
    unsafe { rsf_core_fg12_cpu(&fact) };
    OK
}

fn start_render(timing: &mut Cpu, frame: Frame) {
    if timing.frame == frame && !timing.render_started {
        timing.render_started = true;
        unsafe { native!(latency_render(frame.id, 1)) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fg12_render_begin(session: u64, id: u64) -> i32 {
    if session == 0 || id == 0 {
        return INVALID;
    }
    let frame = Frame { session, id };
    let mut owner = OWNER.lock().unwrap();
    let timing = &mut owner.cpu[frame.index(CPU_SLOTS)];
    if timing.frame != frame || !timing.begun {
        return NOT_READY;
    }
    start_render(timing, frame);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_fg12_capture(
    list: *mut c_void,
    record: *const rsf_frame_record,
    depth: *mut c_void,
    motion: *mut c_void,
) {
    let Some(record) = (unsafe { checked(record) }) else {
        return;
    };
    if record.flags & RSF_FRAME_FLAG_AMBIGUOUS_ID != 0 {
        return;
    }

    let frame = Frame {
        session: record.session_id,
        id: record.frame_id,
    };
    if list.is_null()
        || depth.is_null()
        || motion.is_null()
        || frame.id == 0
        || record.abi_version != RSF_GAME_FRAME_ABI_VERSION
        || unsafe { native!(has_owner()) } == 0
    {
        return;
    }
    let Some(source_queue) = (unsafe { ComRef::from_owned(native!(source_queue())) }) else {
        return;
    };
    let mut owner = OWNER.lock().unwrap();
    let generation = unsafe { native!(present_generation()) };
    let slot = &mut owner.slots[frame.index(INPUT_SLOTS)];
    if let Some(previous) = slot {
        if (previous.record.session_id, previous.record.frame_id) == (frame.session, frame.id) {
            // Multiple primary evaluations require an explicit output association.
            previous.record.flags |= RSF_FRAME_FLAG_RESET;
            return;
        }
        if !previous.reusable(generation) {
            return;
        }
    }
    // A drained switch can release an unknown vendor use, but cannot establish
    // that a failed/aborted preparation list executed all of its barriers.
    let reuse = slot.as_ref().filter(|slot| {
        !matches!(
            slot.vendor,
            VendorUse::Unfenced(_) | VendorUse::Preparing(_)
        ) && slot.source_queue.as_ptr() == source_queue.as_ptr()
    });
    let Some(mut depth_copy) =
        (unsafe { Texture::allocate(list, depth, reuse.map(|slot| &slot.depth)) })
    else {
        return;
    };
    let Some(mut motion_copy) =
        (unsafe { Texture::allocate(list, motion, reuse.map(|slot| &slot.motion)) })
    else {
        return;
    };
    let hudless = if reuse.is_some() {
        slot.as_mut().and_then(|slot| slot.hudless.take())
    } else {
        None
    };
    // Both targets are owned before either command records a GPU reference.
    unsafe {
        depth_copy.copy_from(list, depth, SHADER_READ);
        motion_copy.copy_from(list, motion, SHADER_READ);
    }
    *slot = Some(Slot {
        record: *record,
        depth: depth_copy,
        motion: motion_copy,
        hudless,
        source_queue,
        source_fence: None,
        submitted: false,
        pending_copy: true,
        hudless_recorded: false,
        vendor: VendorUse::None,
    });
    start_render(&mut owner.cpu[frame.index(CPU_SLOTS)], frame);
}

// The D3D12 SR owner hands its accepted normalized depth and motion over inside the core after
// binding the platform table. An unbound table means no presentation owner can exist.
pub(crate) unsafe fn handoff_inputs(
    list: *mut c_void,
    record: &rsf_frame_record,
    depth: *mut c_void,
    motion: *mut c_void,
) {
    if PLATFORM.get().is_some() {
        unsafe { rsf_core_fg12_capture(list, record, depth, motion) };
    }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_hudless(
    list: *mut c_void,
    pass: *const rsf_game_render_pass,
) {
    let Some(pass) = (unsafe { checked(pass) }) else {
        return;
    };
    if list.is_null() || pass.color_input.is_null() || unsafe { native!(depth_domains(pass)) } != 0
    {
        return;
    }
    let view = View::pass(pass);
    if let Some(record) = current_dump(view) {
        unsafe {
            native!(dump_record(
                list,
                pass.color_input,
                RENDER_TARGET,
                &record,
                c"hudless_color".as_ptr(),
                0,
            ));
            if !pass.ui_input.is_null() {
                for role in [c"hud_color", c"hud_alpha"] {
                    native!(dump_record(
                        list,
                        pass.ui_input,
                        SHADER_READ,
                        &record,
                        role.as_ptr(),
                        0,
                    ));
                }
            }
        }
    }
    if unsafe { native!(has_owner()) } == 0 {
        return;
    }
    let mut owner = OWNER.lock().unwrap();
    let Some(slot) = owner.slots[view.frame.index(INPUT_SLOTS)].as_mut() else {
        return;
    };
    if !view.matches(&slot.record) || !matches!(slot.vendor, VendorUse::None) {
        return;
    }
    if slot.hudless_recorded {
        slot.record.flags |= RSF_FRAME_FLAG_RESET;
        return;
    }
    let desc = unsafe { texture_desc(pass.color_input) };
    if desc.width != u64::from(slot.record.output_width) || desc.height != slot.record.output_height
    {
        return;
    }
    // An old allocation can be cached, but only a copy from this frame can be tagged.
    let Some(mut hudless) =
        (unsafe { Texture::allocate(list, pass.color_input, slot.hudless.as_ref()) })
    else {
        return;
    };
    unsafe { hudless.copy_from(list, pass.color_input, RENDER_TARGET) };
    slot.hudless = Some(hudless);
    slot.hudless_recorded = true;
    slot.pending_copy = true;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_window(pass: *const rsf_game_render_pass) {
    let Some(pass) = (unsafe { checked(pass) }) else {
        return;
    };
    if unsafe { native!(depth_domains(pass)) != 0 || native!(is_owner(pass.swapchain)) == 0 } {
        return;
    }
    let index = unsafe { native!(buffer_index(pass.swapchain)) } as usize;
    let view = View::pass(pass);
    let mut owner = OWNER.lock().unwrap();
    let Some(window) = owner.windows.get_mut(index) else {
        return;
    };
    *window = view;
    start_render(&mut owner.cpu[view.frame.index(CPU_SLOTS)], view.frame);
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_submitted(session: u64, id: u64) {
    if id == 0 {
        return;
    }
    let frame = Frame { session, id };
    let mut owner = OWNER.lock().unwrap();
    let Owner {
        slots,
        source_fence,
        ..
    } = &mut *owner;
    let Some(slot) = slots[frame.index(INPUT_SLOTS)].as_mut() else {
        return;
    };
    if (slot.record.session_id, slot.record.frame_id) != (session, id)
        || !matches!(slot.vendor, VendorUse::None)
        || (!slot.pending_copy && slot.source_fence.is_some())
    {
        return;
    }
    slot.submitted = true;
    slot.pending_copy = false;
    let queue = &slot.source_queue;
    if source_fence
        .as_ref()
        .is_none_or(|fence| queue.as_ptr() != fence.queue.as_ptr())
    {
        let fence = unsafe { ComRef::from_owned(native!(fence(queue.as_ptr()))) };
        *source_fence = fence.map(|fence| SourceFence {
            queue: queue.clone(),
            fence,
            serial: 0,
        });
    }
    // Confirmation makes the inputs eligible through the bridge's existing
    // queue ordering. A failed signal still prevents independent slot reuse.
    slot.source_fence = source_fence
        .as_mut()
        .and_then(|fence| unsafe { fence.signal() });
}

unsafe fn texture_desc(resource: *mut c_void) -> rsf_fg12_texture_desc {
    let mut desc = std::mem::MaybeUninit::uninit();
    unsafe {
        native!(texture_desc(resource, desc.as_mut_ptr()));
        desc.assume_init()
    }
}

fn current_dump(view: View) -> Option<rsf_frame_record> {
    let mut record = empty_record();
    (unsafe { native!(dump_current(&mut record)) } != 0 && view.matches(&record)).then_some(record)
}

unsafe fn record_final(view: View, requested: bool, list: *mut c_void, buffer: *mut c_void) {
    if requested && !buffer.is_null() && view.frame.session != 0 && view.frame.id != 0 {
        let mut current = empty_record();
        if unsafe { native!(dump_current(&mut current)) } == 0 {
            let desc = unsafe { texture_desc(buffer) };
            let record = rsf_frame_record {
                abi_version: RSF_GAME_FRAME_ABI_VERSION,
                session_id: view.frame.session,
                frame_id: view.frame.id,
                view_id: view.key,
                output_width: desc.width as u32,
                output_height: desc.height,
                flags: RSF_FRAME_FLAG_NO_SR | RSF_FRAME_FLAG_NO_FG,
                ..empty_record()
            };
            unsafe { native!(dump_begin_record(&record, 0)) };
        }
    }
    if !list.is_null()
        && !buffer.is_null()
        && let Some(record) = current_dump(view)
    {
        unsafe {
            native!(dump_record(
                list,
                buffer,
                PRESENT_STATE,
                &record,
                c"final_color".as_ptr(),
                (view.frame.id % 3) as u32,
            ));
        }
    }
}

fn cpu_allows_fg(frame: Frame, timing: Cpu, generation: u64) -> bool {
    if timing.frame != frame
        || timing.generation != generation
        || !timing.begun
        || timing.failed
        || !latency_frame(frame.id)
            .is_some_and(|core| core.generation == generation && core.failed == 0)
    {
        return false;
    }
    let mut facts = rsf_native_cpu_frame {
        struct_size: size_of::<rsf_native_cpu_frame>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    (unsafe { rsf_native_cpu_read(frame.session, frame.id, &mut facts) != 0 })
        && facts.stage_mask & crate::native_cpu::THROUGH_SIM_END
            == crate::native_cpu::THROUGH_SIM_END
        && facts.failed == 0
        && facts.flags & RSF_GAME_CPU_INPUT_SUBSTITUTE == 0
}

fn eligibility(slot: Option<&Slot>, view: View, cpu_valid: bool, needs_hudless: bool) -> u32 {
    let Some(slot) = slot.filter(|slot| slot.eligible(view) && cpu_valid) else {
        return RSF_NATIVE_FG_REASON_NOT_PREPARED;
    };
    if slot.record.flags & RSF_FRAME_FLAG_RESET != 0 {
        RSF_NATIVE_FG_REASON_RESET
    } else if unsafe { native!(allows_fg(&slot.record)) } == 0 {
        RSF_NATIVE_FG_REASON_SCENE_UNSUPPORTED
    } else if needs_hudless && !slot.hudless_recorded {
        RSF_NATIVE_FG_REASON_HUDLESS_MISSING
    } else {
        0
    }
}

fn resource(
    pointer: *mut c_void,
    generation: u32,
    size: [u32; 2],
    state: u32,
) -> rsf_backend_resource {
    rsf_backend_resource {
        struct_size: size_of::<rsf_backend_resource>() as u32,
        resource: pointer,
        state,
        width: size[0],
        height: size[1],
        generation,
        ..unsafe { std::mem::zeroed() }
    }
}

unsafe extern "C" fn prepare(
    _: *mut c_void,
    chain: *mut c_void,
    _: *mut c_void,
    list: *mut c_void,
    buffer: *mut c_void,
    host: *mut rsf_streamline_host,
    context: *mut c_void,
    _: u32,
) {
    unsafe { native!(dump_flush()) };
    let requested = unsafe { native!(dump_requested()) } != 0;
    unsafe { native!(dump_poll()) };
    let mut owner = OWNER.lock().unwrap();
    let index = unsafe { native!(buffer_index(chain)) } as usize;
    let view = owner
        .windows
        .get_mut(index)
        .map(std::mem::take)
        .unwrap_or_default();
    let generation = unsafe { native!(present_generation()) };
    owner.colors.poll(generation);
    let mut present = Present {
        view,
        generation,
        prepared: false,
    };
    PRESENT.set(present);
    unsafe { record_final(view, requested, list, buffer) };

    let frame = view.frame;
    let timing = owner.cpu[frame.index(CPU_SLOTS)];
    if !host.is_null() {
        crate::native_fg_cpu::retire_unsubmitted(frame.session, frame.id, |id| unsafe {
            native!(host_abort(host, id))
        });
    }

    let Some(provider) =
        (unsafe { native!(present_provider()).as_ref() }).filter(|_| !context.is_null())
    else {
        owner.state.vendor = unsafe { std::mem::zeroed() };
        owner.state.last_result = OK;
        owner.state.source_frame_id = frame.id;
        owner.state.reason = RSF_NATIVE_FG_REASON_NOT_PREPARED;
        return;
    };
    if owner.generation != generation {
        owner.generation = generation;
        owner.samples = 0;
        owner.prepare_failures = 0;
    }
    let mut capability = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    let status_result = provider.status.map_or(NOT_SUPPORTED, |status| unsafe {
        status(context, &mut capability)
    });
    let (OK, Some(configure), Some(tag)) = (status_result, provider.configure, provider.prepare)
    else {
        owner.state.last_result = if status_result == OK {
            NOT_SUPPORTED
        } else {
            status_result
        };
        owner.state.source_frame_id = frame.id;
        owner.state.reason = RSF_NATIVE_FG_REASON_NOT_PREPARED;
        owner.state.vendor = unsafe { std::mem::zeroed() };
        if let Some(configure) = provider.configure {
            unsafe { configure(context, &crate::d3d11_present::FG_OFF) };
        }
        return;
    };
    let slot_index = frame.index(INPUT_SLOTS);
    let slot = owner.slots[slot_index].as_ref();
    let needs_hudless = owner.ui_mode != RSF_UI_MODE_NONE;
    let valid = !list.is_null() && !buffer.is_null() && cpu_allows_fg(frame, timing, generation);
    let reason = eligibility(slot, view, valid, needs_hudless);
    let options = owner.options;
    unsafe { crate::frame_limit::observe_pacing_display(options.frame_limit_us, chain) };
    let mut request = crate::native_fg::provider_options(&options, &capability, reason == 0);
    let result = unsafe { configure(context, &request) };
    owner.state.last_result = result;
    owner.state.source_frame_id = frame.id;
    owner.state.reason = reason;
    owner.state.vendor = capability;
    if request.mode == RSF_FG_OFF || result != OK {
        crate::fg_status::deactivate(&mut owner.state.vendor);
    }
    owner.diagnostics += 1;
    if owner.diagnostics <= 12 || owner.diagnostics.is_multiple_of(600) {
        log(format!(
            "Native renderer FG source={} view={} cpu={} simulation={} generation={}/{} reason={} configure={}",
            frame.id,
            view.key,
            timing.begun,
            timing.simulation,
            timing.generation,
            generation,
            reason,
            result
        ));
    }
    if result != OK || reason != 0 {
        return;
    }
    let samples = owner.samples;
    let Owner { slots, colors, .. } = &mut *owner;
    let Some(slot) = slots[slot_index].as_mut() else {
        return;
    };
    let record = rsf_frame_record {
        input_qpc: timing.input,
        phase: UI_COMPLETE,
        ..slot.record
    };
    let render_size = [record.render_width, record.render_height];
    let output_size = [record.output_width, record.output_height];
    let resource_generation = record.resource_generation;
    let want_pair = slot.hudless_recorded && colors.wants(present, samples);
    let mut submission = rsf_fg_frame {
        struct_size: size_of::<rsf_fg_frame>() as u32,
        record: &record,
        backbuffer: resource(buffer, resource_generation, output_size, PRESENT_STATE),
        depth: resource(
            unsafe { slot.depth.read(list) },
            resource_generation,
            render_size,
            SHADER_READ,
        ),
        motion: resource(
            unsafe { slot.motion.read(list) },
            resource_generation,
            render_size,
            SHADER_READ,
        ),
        interpolate: u32::from(options.mode != RSF_FG_OFF),
        motion_scale_x: 1.0,
        motion_scale_y: 1.0,
        ..unsafe { std::mem::zeroed() }
    };
    if let Some(hudless) = slot.hudless.as_mut().filter(|_| needs_hudless || want_pair) {
        let raw = unsafe { hudless.read(list) };
        if needs_hudless {
            submission.hudless = resource(raw, resource_generation, output_size, SHADER_READ);
        }
        if want_pair {
            unsafe { colors.record(present, list, buffer, raw) };
        }
    }
    // A provider can record barriers or an intermediate dispatch before it
    // reports a failure. Both successful and failed attempts need retirement.
    slot.vendor = VendorUse::Preparing(generation);
    present.prepared = true;
    PRESENT.set(present);
    let result = unsafe { tag(context, list, &submission) };
    let backend = unsafe { native!(present_backend()) };
    unsafe {
        native!(dump_prepare(&submission, result));
        native!(dump_result(&record, c"fg".as_ptr(), backend, result));
    }
    owner.state.last_result = result;
    if result == OK {
        owner.state.tagged_frames += u64::from(submission.interpolate != 0);
    } else {
        if owner.prepare_failures < 8 {
            owner.prepare_failures += 1;
            log(format!(
                "Native renderer FG prepare backend={} source={} result={} render={}x{} output={}x{}",
                backend,
                frame.id,
                result,
                render_size[0],
                render_size[1],
                output_size[0],
                output_size[1]
            ));
        }
        request.mode = RSF_FG_OFF;
        unsafe { configure(context, &request) };
        crate::fg_status::deactivate(&mut owner.state.vendor);
    }
}

unsafe extern "C" fn latency(_: *mut c_void, event: *const rsf_observer_present_event) {
    let event = unsafe { &*event };
    let present = PRESENT.get();
    let frame = present.view.frame;
    if frame.id == 0 || event.flags & PRESENT_TEST != 0 {
        return;
    }
    if event.completed == 0 {
        unsafe { native!(latency_present(frame.id, event.swapchain, 1)) };
        return;
    }
    let queue = unsafe { native!(present_queue()) };
    if let Some(record) = current_dump(present.view) {
        unsafe {
            native!(dump_submitted(queue, &record));
            native!(dump_complete(&record, event.result));
        }
    }
    unsafe {
        native!(latency_present(frame.id, event.swapchain, 0));
    }
    if !queue.is_null() {
        unsafe { OWNER.lock().unwrap().colors.submitted(present, queue) };
    }
}

unsafe extern "C" fn retire(_: *mut c_void, context: *mut c_void) {
    let present = PRESENT.replace(Present::default());
    let mut owner = OWNER.lock().unwrap();
    let generation = unsafe { native!(present_generation()) };
    let Some(provider) =
        (unsafe { native!(present_provider()).as_ref() }).filter(|_| !context.is_null())
    else {
        owner.state.vendor = unsafe { std::mem::zeroed() };
        return;
    };
    let mut status = rsf_fg_status {
        struct_size: size_of::<rsf_fg_status>() as u32,
        ..unsafe { std::mem::zeroed() }
    };
    if provider
        .status
        .is_some_and(|read| unsafe { read(context, &mut status) } == OK)
    {
        if owner.state.reason != 0
            || owner.state.last_result != OK
            || owner.options.mode == RSF_FG_OFF
        {
            crate::fg_status::deactivate(&mut status);
        }
        owner.state.vendor = status;
        owner.samples += 1;
        if owner.samples <= 3 || owner.samples.is_multiple_of(120) {
            log(format!(
                "Native renderer FG status backend={} source={} prepare={} mode={} active={} vendor={} generated_callbacks={} presented={} valid_stats=0x{:x}",
                unsafe { native!(present_backend()) },
                present.view.frame.id,
                owner.state.last_result,
                status.effective_mode,
                status.active,
                status.vendor_status,
                status.generated_callbacks,
                status.total_presented,
                status.valid_statistics
            ));
        }
    }
    if present.generation != generation {
        return;
    }
    // Even without tags this callback can settle an earlier failed retirement.
    let wanted = owner.slots.iter().flatten().any(|slot| {
        slot.vendor.needs_join(
            generation,
            present.prepared && present.view.matches(&slot.record),
        )
    });
    if !wanted {
        return;
    }
    drop(owner);

    // A vendor retirement can depend on asynchronously presented frames. CPU
    // callbacks and rendering keep progressing while the bridge joins it.
    let mut fence = null_mut();
    let mut value = 0;
    let result = unsafe { native!(present_retire(context, null_mut(), &mut fence, &mut value)) };
    let point = if result == OK && !fence.is_null() && value != 0 && value != u64::MAX {
        // The bridge returns a borrowed fence inside the Present callback lease.
        Some(FencePoint {
            fence: unsafe { ComRef::retain(fence) },
            value,
        })
    } else {
        None
    };
    let mut owner = OWNER.lock().unwrap();
    for slot in owner.slots.iter_mut().flatten() {
        if slot.vendor.needs_join(
            generation,
            present.prepared && present.view.matches(&slot.record),
        ) {
            slot.vendor = point
                .clone()
                .map_or(VendorUse::Unfenced(generation), VendorUse::Joined);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_fg12_end_session(session: u64) -> i32 {
    if session == 0 {
        return 0;
    }
    let mut owner = OWNER.lock().unwrap();
    let generation = unsafe { native!(present_generation()) };
    if owner.slots.iter().flatten().any(|slot| {
        slot.record.session_id == session
            && if matches!(slot.vendor, VendorUse::None) {
                slot.source_fence
                    .as_ref()
                    .is_some_and(|point| point.state() != FenceState::Complete)
            } else {
                !slot.vendor.reusable(generation)
            }
    }) {
        return 0;
    }
    // The caller has quiesced and drained its recording owner. Only the vendor
    // inputs could still be in use after the caller's own GPU fence.
    unsafe { native!(dump_flush()) };
    for frame in &mut owner.cpu {
        if frame.frame.session == session {
            *frame = Cpu::EMPTY;
        }
    }
    for slot in &mut owner.slots {
        if slot
            .as_ref()
            .is_some_and(|slot| slot.record.session_id == session)
        {
            *slot = None;
        }
    }
    if owner.slots.iter().all(Option::is_none) {
        owner.source_fence = None;
    }
    for window in &mut owner.windows {
        if window.frame.session == session {
            *window = View::default();
        }
    }
    owner.colors.poll(generation);
    drop(owner);
    unsafe { native!(latency_end_session(session)) };
    1
}

unsafe fn status_snapshot(output: *mut rsf_native_fg_status, wait: bool) -> i32 {
    if !unsafe { has_capacity(output) } {
        return 0;
    }
    let owner = if wait {
        OWNER.lock().unwrap()
    } else {
        let Ok(owner) = OWNER.try_lock() else {
            return 0;
        };
        owner
    };
    let mut result = owner.state;
    result.struct_size = size_of::<rsf_native_fg_status>() as u32;
    result.requested = owner.options;
    result.available =
        u32::from(unsafe { native!(has_owner()) != 0 && native!(present_backend()) != 0 });
    let generation = owner.generation;
    if generation != unsafe { native!(present_generation()) } {
        result.vendor = unsafe { std::mem::zeroed() };
    }
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..rsf_native_latency_status::default()
    };
    if unsafe { native!(latency_status_get(&mut latency)) } != 0 {
        result.vendor.pacing_owner = latency.owner;
        result.vendor.low_latency_available = latency.available;
        result.vendor.effective_reflex = latency.effective_mode;
        result.vendor.latency_paced_frames = latency.paced_frames;
    }
    unsafe { output.write(result) };
    1
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_status(output: *mut rsf_native_fg_status) -> i32 {
    unsafe { status_snapshot(output, true) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_status_try(output: *mut rsf_native_fg_status) -> i32 {
    unsafe { status_snapshot(output, false) }
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_options(request: *const rsf_native_fg_options) {
    let Some(request) = (unsafe { checked(request) }) else {
        return;
    };
    if !options_valid(request) {
        return;
    }
    unsafe { native!(fg_options_set(request)) };
    OWNER.lock().unwrap().options = *request;
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_core_fg12_capture_outputs(directory: *const c_char) -> i32 {
    if directory.is_null() {
        return INVALID;
    }
    let Ok(directory) = unsafe { CStr::from_ptr(directory) }.to_str() else {
        return INVALID;
    };
    if directory.encode_utf16().count() >= 32_768 {
        return INVALID;
    }
    OWNER.lock().unwrap().colors.directory =
        (!directory.is_empty()).then(|| PathBuf::from(directory));
    OK
}
