// SPDX-License-Identifier: MIT
//! The Fallout 4 CPU event owner: admission of the native poll, consume, PlayerControls, Swap,
//! idle, raw input, worker job, wrapper and join callbacks; the Main poll source and the bounded
//! worker successor handoff; the GameSDK CPU facts they report; the PeekMessageA import cell; and
//! the hook lifecycle.
//!
//! Identity is borrowed during the guarded Swap. A proved PlayerControls consumer may adopt a
//! reserved engine-buffer poll source before Swap; a selected mode1 worker can hand one consumed
//! successor through its native post-Swap join to the next ordinary world. Otherwise Swap opens a
//! rendered source. No simulation or GPU stage is implied. Native RAWINPUT, selected removed
//! window messages and delivered gamepad batches are separate unframed receipts; they never assert
//! a physical sample.
//!
//! The integration reports what the engine did; the Runtime decides pacing, marker meaning and
//! vendors. cpu_events.cpp keeps the typed detour shells, which call each original engine function
//! through its MinHook trampoline on their own C++ frames between the entry and exit decisions
//! here, the guarded host callback and the resident PeekMessageA thunk.
mod handoff;
mod native;

use crate::build_layout::{RENDERER_OBJECT, RENDERER_POINTER, RENDERER_WINDOW};
use crate::cpu_abi::{
    RSF_FALLOUT4_CPU_HOOK_CONSUME, RSF_FALLOUT4_CPU_HOOK_COUNT, RSF_FALLOUT4_CPU_HOOK_IDLE,
    RSF_FALLOUT4_CPU_HOOK_JOB_END, RSF_FALLOUT4_CPU_HOOK_KEYBOARD, RSF_FALLOUT4_CPU_HOOK_MOUSE,
    RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS, RSF_FALLOUT4_CPU_HOOK_POLL, RSF_FALLOUT4_CPU_HOOK_SWAP,
    RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN, RSF_FALLOUT4_CPU_HOOK_WRAPPER, RSF_FALLOUT4_CPU_SCOPE_WORDS,
    rsf_fallout4_cpu_identity, rsf_fallout4_cpu_platform, rsf_fallout4_cpu_scope,
};
use crate::{
    MH_CreateHook, MH_OK, MH_RemoveHook, Ops, Plain, creation, image, message, ops, pin, read,
};
use handoff::{
    CpuIdentity, Phase, WorkerCycle, WorkerTicket, active_join_matches, invalidates_render_source,
};
use native::{InputState, StateRead, load};
use rsf_game_sys::{
    RSF_GAME_ABI_VERSION, RSF_GAME_CPU_ABORT_INTERVENING_POLL, RSF_GAME_CPU_ABORT_NOT_CONSUMED,
    RSF_GAME_CPU_ABORT_OVERLAP, RSF_GAME_CPU_ABORT_QUIESCE, RSF_GAME_CPU_ABORT_STATE_CHANGED,
    RSF_GAME_CPU_EVENT_VERSION, RSF_GAME_CPU_EVENT_VERSION_RECEIPT,
    RSF_GAME_CPU_EVENT_VERSION_SOURCE, RSF_GAME_CPU_FRAME_BEGIN, RSF_GAME_CPU_FRAME_END,
    RSF_GAME_CPU_INPUT_EVENT, RSF_GAME_CPU_INPUT_UNFRAMED, RSF_GAME_CPU_RENDER_SKIPPED,
    RSF_GAME_CPU_SIMULATION_BEGIN, RSF_GAME_CPU_SIMULATION_END, RSF_GAME_CPU_SOURCE_ABORT,
    RSF_GAME_CPU_SOURCE_BIND, RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR,
    RSF_GAME_CPU_SOURCE_PRODUCED, RSF_GAME_CPU_SOURCE_RESERVE, RSF_GAME_CPU_SOURCE_SAMPLED,
    RSF_GAME_CPU_WINDOW_MESSAGE, RSF_GAME_INPUT_CONTROLLER, RSF_GAME_INPUT_KEYBOARD,
    RSF_GAME_INPUT_MOUSE, rsf_game_cpu_event, rsf_game_cpu_receipt_event,
    rsf_game_cpu_source_event, rsf_game_host_services,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use windows_sys::Win32::Foundation::{GetLastError, SetLastError};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, VirtualProtect,
    VirtualQuery,
};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, PM_REMOVE};

use Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst};

// SAFETY: MSG holds integers and a window handle; every bit pattern is a value.
unsafe impl Plain for MSG {}

const WAIT_OBJECT_0: u32 = 0;

// ---------------------------------------------------------------------------------------------
// Native leaves.

struct Leaves {
    cpu_event: unsafe extern "C" fn(
        callback: rsf_game_sys::rsf_game_cpu_event_fn,
        user: *mut ::core::ffi::c_void,
        event: *const rsf_game_cpu_event,
    ) -> bool,
    finish_render_scope: unsafe extern "C" fn(),
    peek_thunk: unsafe extern "C" fn() -> *mut ::core::ffi::c_void,
}

impl Leaves {
    fn from_native(table: &rsf_fallout4_cpu_platform) -> Option<Self> {
        Some(Self {
            cpu_event: table.cpu_event?,
            finish_render_scope: table.finish_render_scope?,
            peek_thunk: table.peek_thunk?,
        })
    }
}

static PLATFORM: OnceLock<Leaves> = OnceLock::new();

fn platform() -> Option<&'static Leaves> {
    PLATFORM.get()
}

/// Binds the CPU leaves for the process.
///
/// # Safety
/// A nonnull `table` is a complete leaf table that lives for the process.
pub(crate) unsafe fn publish(table: *const rsf_fallout4_cpu_platform) -> bool {
    let Some(table) = (unsafe { table.as_ref() }).and_then(Leaves::from_native) else {
        return false;
    };
    let _ = PLATFORM.get_or_init(|| table);
    true
}

fn hook_target(hook: u32) -> *mut c_void {
    (image() + native::HOOK_ENTRIES[hook as usize].0) as *mut c_void
}

// Hook lifecycle is serialized by Session. The detours only load the published originals.
static ORIGINALS: [AtomicPtr<c_void>; RSF_FALLOUT4_CPU_HOOK_COUNT as usize] =
    [const { AtomicPtr::new(std::ptr::null_mut()) }; RSF_FALLOUT4_CPU_HOOK_COUNT as usize];

unsafe extern "C" {
    fn rsf_fallout4_cpu_detour(hook: u32) -> *mut c_void;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_original(hook: u32) -> *mut c_void {
    ORIGINALS[hook as usize].load(Acquire)
}

fn hook_created(hook: u32) -> bool {
    !rsf_fallout4_cpu_original(hook).is_null()
}

fn create_hook(hook: u32, target: *mut c_void) -> bool {
    let mut original = std::ptr::null_mut();
    let detour = unsafe { rsf_fallout4_cpu_detour(hook) };
    // MinHook creates this disabled. Publish the trampoline before the later start.
    if unsafe { MH_CreateHook(target, detour, &mut original) } != MH_OK {
        return false;
    }
    ORIGINALS[hook as usize].store(original, Release);
    true
}

fn remove_hook(hook: u32, target: *mut c_void) -> bool {
    if unsafe { MH_RemoveHook(target) } != MH_OK {
        return false;
    }
    ORIGINALS[hook as usize].store(std::ptr::null_mut(), Release);
    true
}

fn peek_thunk() -> usize {
    platform().map_or(0, |table| unsafe { (table.peek_thunk)() } as usize)
}

fn finish_render_scope() {
    if let Some(table) = platform() {
        unsafe { (table.finish_render_scope)() };
    }
}

// ---------------------------------------------------------------------------------------------
// Process state.

#[derive(Clone, Copy)]
struct Host(rsf_game_host_services);
// SAFETY: process-lived Runtime services, written by prepare before any hook is enabled.
unsafe impl Send for Host {}

static HOST: Mutex<Host> = Mutex::new(Host(unsafe { std::mem::zeroed() }));

fn host() -> rsf_game_host_services {
    HOST.lock().unwrap_or_else(PoisonError::into_inner).0
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static ENTERED: AtomicU32 = AtomicU32::new(0);
static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
static NEXT_SUBMISSION: AtomicU64 = AtomicU64::new(1);
static NEXT_RECEIPT: AtomicU64 = AtomicU64::new(1);
static POLL_GENERATION: AtomicU64 = AtomicU64::new(0);
static RAW_REVISION: AtomicU64 = AtomicU64::new(0);
static POLLS: AtomicU32 = AtomicU32::new(0);
static RAW_WRITERS: AtomicU32 = AtomicU32::new(0);
static JOBS: AtomicU32 = AtomicU32::new(0);
// Process-wide log budgets and first-seen fact sets. No event or core acknowledgement is
// inferred from any of them.
static SOURCE_RECEIPTS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static SOURCE_RECEIPT_BUDGET: AtomicU32 = AtomicU32::new(0);
static NOTIFICATION_RECEIPTS: [AtomicU32; 2] = [const { AtomicU32::new(0) }; 2];
static GATE_RECEIPTS: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];
static DOWNSTREAM_RECEIPTS: AtomicU32 = AtomicU32::new(0);
static CYCLE_STAGES: [AtomicU64; 15] = [const { AtomicU64::new(0) }; 15];
static CYCLE_BUDGET: AtomicU32 = AtomicU32::new(0);
static POLL_COMBINATIONS: [AtomicU64; 32] = [const { AtomicU64::new(0) }; 32];

fn enabled() -> bool {
    ENABLED.load(SeqCst)
}

fn qpc() -> u64 {
    let mut value = 0i64;
    unsafe { QueryPerformanceCounter(&mut value) };
    value as u64
}

fn qpc_frequency() -> u64 {
    let mut value = 0i64;
    unsafe { QueryPerformanceFrequency(&mut value) };
    value as u64
}

fn thread_id() -> u32 {
    unsafe { GetCurrentThreadId() }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn set_last_error(error: u32) {
    unsafe { SetLastError(error) };
}

fn predicate_bits(terms: &[bool]) -> u32 {
    terms
        .iter()
        .enumerate()
        .filter(|&(_, &term)| term)
        .fold(0, |bits, (index, _)| bits | 1 << index)
}

fn log(host: &rsf_game_host_services, text: String) {
    if let Some(ops) = ops() {
        let text = message(text);
        let _ = ops.log_host(host, &text);
    }
}

/// A pointer as the previous native MSVC %p rendering.
struct P(usize);
impl std::fmt::Display for P {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016X}", self.0)
    }
}

// The Main poll source and the source of the current Swap.
#[derive(Clone, Copy)]
struct Pending {
    source: u64,
    generation: u64,
    raw: u64,
    sample_begin: u64,
    frequency: u64,
    queue_generation: u32,
    state: InputState,
    sampled: bool,
    bound: bool,
    simulation_started: bool,
    predecessor: u64,
}

impl Pending {
    const ZERO: Self = Self {
        source: 0,
        generation: 0,
        raw: 0,
        sample_begin: 0,
        frequency: 0,
        queue_generation: 0,
        state: InputState::ZERO,
        sampled: false,
        bound: false,
        simulation_started: false,
        predecessor: 0,
    };
}

#[derive(Clone, Copy)]
struct RenderCycle {
    key: WorkerCycle,
    cpu: CpuIdentity,
    binding: Pending,
    state: InputState,
    tid: u32,
    constructed: bool,
    sealed: bool,
    present: bool,
    swapped: bool,
    joining: bool,
    closed: bool,
    invalid: bool,
    mapped: bool,
    abort_pending: bool,
    aborting: bool,
}

impl RenderCycle {
    const ZERO: Self = Self {
        key: WorkerCycle::ZERO,
        cpu: CpuIdentity::ZERO,
        binding: Pending::ZERO,
        state: InputState::ZERO,
        tid: 0,
        constructed: false,
        sealed: false,
        present: false,
        swapped: false,
        joining: false,
        closed: false,
        invalid: false,
        mapped: false,
        abort_pending: false,
        aborting: false,
    };
}

#[derive(Clone, Copy)]
struct Future {
    facts: WorkerTicket,
    state: InputState,
    busy: bool,
    retiring: bool,
    abort_sent: bool,
    reserve_announced: bool,
    simulation_started: bool,
}

impl Future {
    const ZERO: Self = Self {
        facts: WorkerTicket::ZERO,
        state: InputState::ZERO,
        busy: false,
        retiring: false,
        abort_sent: false,
        reserve_announced: false,
        simulation_started: false,
    };
}

struct Worker {
    current: RenderCycle,
    future: Future,
}

// Serializes only the scalar render and future transitions. It is never held while delivering
// events, logging, reading native memory or calling native code.
static WORKER: Mutex<Worker> = Mutex::new(Worker {
    current: RenderCycle::ZERO,
    future: Future::ZERO,
});

fn worker() -> MutexGuard<'static, Worker> {
    WORKER.lock().unwrap_or_else(PoisonError::into_inner)
}

fn render_copy() -> RenderCycle {
    worker().current
}

fn future_copy() -> Future {
    worker().future
}

#[derive(Clone, Copy)]
struct Delivery {
    queue: usize,
    receiver: usize,
}

const NO_DELIVERY: Delivery = Delivery {
    queue: 0,
    receiver: 0,
};

thread_local! {
    static IDENTITY: Cell<CpuIdentity> = const { Cell::new(CpuIdentity::ZERO) };
    static RENDERED: Cell<u32> = const { Cell::new(0) };
    static IDLE_MAIN: Cell<usize> = const { Cell::new(0) };
    static IDLE_DEPTH: Cell<u32> = const { Cell::new(0) };
    static SWAP_DEPTH: Cell<u32> = const { Cell::new(0) };
    static SOURCE_INVALID: Cell<bool> = const { Cell::new(false) };
    static SWAP_ADMITTED: Cell<bool> = const { Cell::new(false) };
    static CONSUMER_ADMITTED: Cell<bool> = const { Cell::new(false) };
    static DELIVERY: Cell<Delivery> = const { Cell::new(NO_DELIVERY) };
    static PENDING: Cell<Pending> = const { Cell::new(Pending::ZERO) };
    static IDLE_SERIAL: Cell<u64> = const { Cell::new(0) };
    static WORKER_SOURCE: Cell<u64> = const { Cell::new(0) };
    static JOB_SELECTED: Cell<bool> = const { Cell::new(false) };
    static WRAPPER_SELECTED: Cell<bool> = const { Cell::new(false) };
    static FUTURE_CLAIMED: Cell<bool> = const { Cell::new(false) };
    static JOB_DEPTH: Cell<u32> = const { Cell::new(0) };
    static WRAPPER_DEPTH: Cell<u32> = const { Cell::new(0) };
}

fn pending() -> Pending {
    PENDING.get()
}

fn update_pending(change: impl FnOnce(&mut Pending)) {
    let mut value = PENDING.get();
    change(&mut value);
    PENDING.set(value);
}

/// The snapshot for `expected_main`, or for this thread's idle Main when zero.
fn snapshot(value: &mut InputState, expected_main: usize) -> StateRead {
    let main = if expected_main != 0 {
        expected_main
    } else {
        IDLE_MAIN.get()
    };
    native::state(value, main)
}

fn eligible(value: &mut InputState, expected_main: usize) -> bool {
    snapshot(value, expected_main) == StateRead::Eligible
}

// ---------------------------------------------------------------------------------------------
// The native worker cycle proof.

#[derive(Clone, Copy)]
#[repr(u32)]
enum CyclePhase {
    Other,
    BoundCurrent,
    ReserveEntry,
    ReserveReturn,
    Sampled,
    ConsumerBegin,
    FrameReturn,
    BindReturn,
    ConsumerReturn,
    JobComplete,
    SwapClaim,
    JoinEntry,
    JoinReturn,
    WorldAdopt,
    Present,
}

const CYCLE_PHASES: [&str; 15] = [
    "other",
    "bound-current",
    "reserve-entry",
    "reserve-return",
    "sampled",
    "consumer-begin",
    "frame-return",
    "bind-return",
    "consumer-return",
    "job-complete",
    "swap-claim",
    "join-entry",
    "join-return",
    "world-adopt",
    "present",
];

/// Reached and passed proof terms by bit.
#[derive(Default)]
struct Terms {
    reached: u32,
    passed: u32,
    first: u32,
}

impl Terms {
    fn test(&mut self, index: u32, value: bool) -> bool {
        self.reached |= 1 << index;
        if value {
            self.passed |= 1 << index;
        } else {
            self.first = index;
        }
        value
    }

    fn known(&self, index: u32) -> u32 {
        u32::from(self.passed & (1 << index) != 0)
    }
}

/// Proves the selected native worker, its Main/input owner, renderer window and native frame.
/// Each term keeps the original evaluation and read order and short circuit.
fn native_cycle(
    proof: &mut WorkerCycle,
    native: &mut InputState,
    joining: bool,
    phase: CyclePhase,
) -> bool {
    let expected = *proof;
    let base = image();
    let mut t = Terms::default();
    let (mut worker_address, mut vtable, mut destructor, mut entry, mut renderer, mut window) =
        (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut running, mut exiting, mut mode) = (0u8, 0u8, 0u32);
    let state_stage = snapshot(native, proof.main);
    let proved = t.test(0, state_stage == StateRead::Eligible)
        && t.test(1, load(&mut worker_address, base + native::WORKER))
        && t.test(2, worker_address == base + native::WORKER_OBJECT)
        && t.test(3, load(&mut vtable, worker_address))
        && t.test(4, vtable == base + native::WORKER_VTABLE)
        && t.test(5, load(&mut destructor, vtable))
        && t.test(6, destructor == base + native::WORKER_DESTRUCTOR)
        && t.test(7, load(&mut entry, vtable + 8))
        && t.test(8, entry == base + native::WORKER_ENTRY)
        && t.test(
            9,
            load(
                &mut proof.start_event,
                worker_address + native::WORKER_START,
            ),
        )
        && t.test(10, proof.start_event != 0)
        && t.test(
            11,
            load(
                &mut proof.complete_event,
                worker_address + native::WORKER_COMPLETE,
            ),
        )
        && t.test(12, proof.complete_event != 0)
        && t.test(13, load(&mut mode, worker_address + native::WORKER_MODE))
        && t.test(14, mode == 1 || (joining && mode == 0))
        && t.test(
            15,
            load(&mut running, worker_address + native::WORKER_RUNNING),
        )
        && t.test(16, running != 0)
        && t.test(
            17,
            load(&mut exiting, worker_address + native::WORKER_EXITING),
        )
        && t.test(18, exiting == 0)
        && t.test(19, load(&mut window, native.main + native::MAIN_WINDOW))
        && t.test(20, window != 0)
        && t.test(21, load(&mut renderer, base + RENDERER_POINTER))
        && t.test(22, renderer == base + RENDERER_OBJECT)
        && t.test(23, load(&mut proof.window, renderer + RENDERER_WINDOW))
        && t.test(24, proof.window == window)
        && t.test(25, load(&mut proof.frame, base + native::NATIVE_FRAME));
    if proved {
        proof.main = native.main;
        proof.worker = worker_address;
        proof.manager = native.manager;
        proof.queue = native.queue;
        proof.receiver = native.receiver;
        return true;
    }
    // Main publishes joining before native BD51D2 clears mode. A worker snapshot taken before
    // that publication can observe mode0 afterwards. Retry the complete native proof only under
    // the same active join lease.
    if t.first == 14 && mode == 0 && !joining && JOB_DEPTH.get() == 1 && JOB_SELECTED.get() {
        let retry = {
            let w = worker();
            enabled()
                && active_join_matches(
                    &expected,
                    &w.current.key,
                    &w.future.facts.predecessor,
                    WORKER_SOURCE.get(),
                    w.future.facts.cpu.source,
                    w.current.joining,
                    w.current.swapped,
                    w.current.closed,
                    w.current.invalid,
                )
        };
        // Native completion cannot signal until this worker callback returns.
        if retry {
            *proof = expected;
            *native = InputState::ZERO;
            return native_cycle(proof, native, true, phase);
        }
    }
    // No new native reads. A failed read can leave an old value; reached/passed and the known
    // bits, never its scalar alone, establish it.
    let stage = if t.first != 0 {
        t.first
    } else {
        26 + state_stage as u32
    };
    let bit = 1u64 << stage;
    let index = phase as usize;
    let host = host();
    if host.log.is_some()
        && index < CYCLE_STAGES.len()
        && CYCLE_STAGES[index].fetch_or(bit, Relaxed) & bit == 0
        && CYCLE_BUDGET.fetch_add(1, Relaxed) < 32
    {
        let error = last_error();
        let state_known = [
            StateRead::MainRead,
            StateRead::ManagerRead,
            StateRead::PollingRead,
            StateRead::QueueRead,
            StateRead::ReceiverRead,
            StateRead::FlagsRead,
            StateRead::MenuRead,
        ]
        .iter()
        .enumerate()
        .fold(0u32, |bits, (index, &stage)| {
            bits | (u32::from(state_stage > stage) << index)
        });
        let f = native.flags;
        log(
            &host,
            format!(
                "Fallout4 native-cycle refusal phase={} first={} state_stage={} reached=0x{:x} passed=0x{:x} joining={} source={} thread_source={} worker_source={} expected_native={} frame_known={} native={} depths={}/{}/{}/{} state_known=0x{:x} main={} expected_main={} manager={} queue={} receiver={} menu={} polling={} flags={},{},{},{},{},{},{},{} worker_known={} worker={} vtable_known={} vtable={} destructor_known={} destructor={} run_known={} run={} start_known={} start={} complete_known={} complete={} mode_known={} mode={} running_known={} running={} exit_known={} exit={} main_window_known={} main_window={} renderer_known={} renderer={} render_window_known={} render_window={}",
                CYCLE_PHASES[index],
                t.first,
                state_stage as u32,
                t.reached,
                t.passed,
                u32::from(joining),
                expected.source,
                IDENTITY.get().source,
                WORKER_SOURCE.get(),
                expected.frame,
                t.known(25),
                proof.frame,
                IDLE_DEPTH.get(),
                SWAP_DEPTH.get(),
                JOB_DEPTH.get(),
                WRAPPER_DEPTH.get(),
                state_known,
                P(native.main),
                P(expected.main),
                P(native.manager),
                P(native.queue),
                P(native.receiver),
                native.menu,
                native.polling_enabled,
                f[0],
                f[1],
                f[2],
                f[3],
                f[4],
                f[5],
                f[6],
                f[7],
                t.known(1),
                P(worker),
                t.known(3),
                P(vtable),
                t.known(5),
                P(destructor),
                t.known(7),
                P(entry),
                t.known(9),
                P(proof.start_event),
                t.known(11),
                P(proof.complete_event),
                t.known(13),
                mode,
                t.known(15),
                running,
                t.known(17),
                exiting,
                t.known(19),
                P(window),
                t.known(21),
                P(renderer),
                t.known(23),
                P(proof.window),
            ),
        );
        set_last_error(error);
    }
    false
}

fn ticket_pending(value: &Future) -> Pending {
    let f = &value.facts;
    Pending {
        source: f.cpu.source,
        generation: f.generation,
        raw: f.raw,
        sample_begin: f.poll_begin,
        frequency: f.cpu.frequency,
        queue_generation: f.cursor,
        state: value.state,
        sampled: f.phase >= Phase::Sampled,
        bound: f.phase >= Phase::Bound,
        simulation_started: false,
        predecessor: f.predecessor.source,
    }
}

// ---------------------------------------------------------------------------------------------
// GameSDK CPU facts.

fn deliver(host: &rsf_game_host_services, event: *const rsf_game_cpu_event) -> bool {
    match (host.cpu_event, platform()) {
        (Some(callback), Some(table)) => unsafe {
            (table.cpu_event)(Some(callback), host.user, event)
        },
        _ => false,
    }
}

fn empty_event(size: usize, version: u32) -> rsf_game_cpu_event {
    rsf_game_cpu_event {
        struct_size: size as u32,
        stage: 0,
        session_id: 0,
        source_frame_id: 0,
        timestamp_qpc: 0,
        qpc_frequency: 0,
        render_expected: 0,
        input_kind: 0,
        message_id: 0,
        event_version: version,
        flags: 0,
    }
}

/// An unframed native input receipt.
fn received(kind: u32, message_id: u32, window_message: bool, window_key: u64) {
    let now = qpc();
    let frequency = qpc_frequency();
    let host = host();
    let mut header = empty_event(
        size_of::<rsf_game_cpu_receipt_event>(),
        RSF_GAME_CPU_EVENT_VERSION_RECEIPT,
    );
    header.stage = RSF_GAME_CPU_INPUT_EVENT;
    header.session_id = host.session_id;
    header.timestamp_qpc = now;
    header.qpc_frequency = frequency;
    header.input_kind = kind;
    header.flags = RSF_GAME_CPU_INPUT_UNFRAMED;
    if window_message {
        header.flags |= RSF_GAME_CPU_WINDOW_MESSAGE;
    }
    header.message_id = message_id;
    let receipt = rsf_game_cpu_receipt_event {
        header,
        receipt_id: NEXT_RECEIPT.fetch_add(1, Relaxed),
        window_key,
    };
    if host.cpu_event.is_some() {
        let _ = deliver(&host, &receipt.header);
    }
    if (window_message || kind == RSF_GAME_INPUT_CONTROLLER)
        && host.log.is_some()
        && NOTIFICATION_RECEIPTS[usize::from(!window_message)].fetch_add(1, SeqCst) < 4
    {
        log(
            &host,
            format!(
                "Fallout4 input receipt emitted: source=0 kind={kind} window_message={} message={message_id}.",
                u32::from(window_message)
            ),
        );
    }
}

fn emit_identity(source: &CpuIdentity, stage: u32, expected_render: u32) -> bool {
    let now = qpc();
    let mut event = empty_event(size_of::<rsf_game_cpu_event>(), RSF_GAME_CPU_EVENT_VERSION);
    event.stage = stage;
    event.session_id = source.session;
    event.source_frame_id = source.source;
    event.timestamp_qpc = now;
    event.qpc_frequency = source.frequency;
    if stage == RSF_GAME_CPU_FRAME_END {
        event.render_expected = expected_render;
        if expected_render == 0 {
            event.flags = RSF_GAME_CPU_RENDER_SKIPPED;
        }
    }
    let host = host();
    host.cpu_event.is_some() && deliver(&host, &event)
}

fn emit(stage: u32) {
    let _ = emit_identity(&IDENTITY.get(), stage, RENDERED.get());
}

/// Emission only: cpu_event has no return status. The core logs its own acknowledgement.
fn source_fact(fact: &Pending, stage: u32, reason: u32) -> bool {
    let host = host();
    let mut header = empty_event(
        size_of::<rsf_game_cpu_source_event>(),
        RSF_GAME_CPU_EVENT_VERSION_SOURCE,
    );
    header.stage = stage;
    header.session_id = host.session_id;
    header.source_frame_id = fact.source;
    header.timestamp_qpc = qpc();
    header.qpc_frequency = fact.frequency;
    let mut event = rsf_game_cpu_source_event {
        header,
        predecessor_source_id: 0,
        native_generation: fact.generation,
        reason,
        reserved: 0,
        poll_begin_qpc: 0,
    };
    if stage == RSF_GAME_CPU_SOURCE_RESERVE && fact.predecessor != 0 {
        event.header.flags = RSF_GAME_CPU_SOURCE_OVERLAPS_PREDECESSOR;
        event.predecessor_source_id = fact.predecessor;
    }
    if stage == RSF_GAME_CPU_SOURCE_SAMPLED {
        // Delivered kinds are unknown until native event receivers run.
        event.header.flags = RSF_GAME_CPU_SOURCE_PRODUCED;
        event.poll_begin_qpc = fact.sample_begin;
    }
    let delivered = host.cpu_event.is_some() && deliver(&host, &event.header);
    let index = stage.wrapping_sub(RSF_GAME_CPU_SOURCE_RESERVE) as usize;
    if host.log.is_some()
        && index < SOURCE_RECEIPTS.len()
        && (SOURCE_RECEIPTS[index].fetch_add(1, SeqCst) < 8 || fact.source % 128 == 0)
        && SOURCE_RECEIPT_BUDGET.fetch_add(1, SeqCst) < 64
    {
        log(
            &host,
            format!(
                "Fallout4 source fact emitted: stage={stage} source={} generation={} reason={reason} poll_generation={} raw_revision={}.",
                fact.source,
                fact.generation,
                POLL_GENERATION.load(SeqCst),
                RAW_REVISION.load(SeqCst)
            ),
        );
    }
    delivered
}

fn source_event(stage: u32, reason: u32) {
    let _ = source_fact(&pending(), stage, reason);
}

// ---------------------------------------------------------------------------------------------
// Render and future obligations.

fn drain_current_abort() -> bool {
    let binding = {
        let mut w = worker();
        if !w.current.abort_pending {
            return true;
        }
        if w.current.aborting {
            return false;
        }
        w.current.aborting = true;
        w.current.binding
    };
    let sent = source_fact(
        &binding,
        RSF_GAME_CPU_SOURCE_ABORT,
        RSF_GAME_CPU_ABORT_STATE_CHANGED,
    );
    let mut w = worker();
    if w.current.binding.source == binding.source {
        w.current.aborting = false;
        if sent {
            w.current.abort_pending = false;
        }
    }
    sent
}

fn drain_future() -> bool {
    let copy = {
        let mut w = worker();
        let f = &mut w.future;
        if !f.facts.active() {
            return true;
        }
        if f.facts.refusal == 0 || f.retiring || (f.busy && !f.facts.callback_inflight) {
            return false;
        }
        f.retiring = true;
        *f
    };
    let aborted = !copy.reserve_announced
        || copy.abort_sent
        || source_fact(
            &ticket_pending(&copy),
            RSF_GAME_CPU_SOURCE_ABORT,
            copy.facts.refusal,
        );
    // Cancellation must reach a synchronous RESERVE callback even while it waits for Main. Keep
    // the ticket until the worker has actually returned.
    let ended = !copy.busy
        && aborted
        && (!copy.facts.frame_begun || emit_identity(&copy.facts.cpu, RSF_GAME_CPU_FRAME_END, 0));
    let mut w = worker();
    if w.future.facts.cpu.source == copy.facts.cpu.source {
        if ended {
            w.future = Future::ZERO;
        } else {
            w.future.abort_sent = aborted;
            w.future.retiring = false;
        }
    }
    ended
}

fn invalidate_worker(reason: u32, invalidate_current: bool, poll_owner: Option<&WorkerCycle>) {
    {
        let mut w = worker();
        w.future.facts.refuse(reason);
        let poll_only = poll_owner.is_some_and(|owner| *owner == w.current.key)
            && enabled()
            && RAW_WRITERS.load(SeqCst) == 0
            && POLLS.load(SeqCst) == 1;
        let c = &mut w.current;
        if invalidate_current
            && c.constructed
            && !c.closed
            && !c.invalid
            && invalidates_render_source(c.binding.source, poll_only)
        {
            c.invalid = true;
            c.abort_pending = c.binding.source != 0;
        }
    }
    let _ = drain_current_abort();
    let _ = drain_future();
}

fn bound_current_valid(c: &RenderCycle, strict_raw: bool) -> bool {
    let mut native = InputState::ZERO;
    let mut cycle_state = InputState::ZERO;
    let (mut cursor, mut queued) = (0u32, 0u32);
    let state_valid = eligible(&mut native, c.key.main) && native == c.state;
    let cursor_read = state_valid && load(&mut cursor, native.receiver + native::RECEIVER_CURSOR);
    let mut proof = c.key;
    let cycle_valid = native_cycle(
        &mut proof,
        &mut cycle_state,
        c.joining,
        CyclePhase::BoundCurrent,
    ) && proof == c.key
        && cycle_state == c.state;
    let queue_read = cycle_valid && load(&mut queued, cycle_state.queue + native::QUEUE_SEQUENCE);
    let w = worker();
    let current = &w.current;
    if !enabled()
        || current.key != c.key
        || current.invalid
        || current.binding.source != c.binding.source
        || !state_valid
    {
        return false;
    }
    if current.sealed && current.mapped {
        return true;
    }
    let generation = POLL_GENERATION.load(SeqCst);
    let revision = RAW_REVISION.load(SeqCst);
    if POLLS.load(SeqCst) == 0
        && generation == c.binding.generation
        && cursor_read
        && cursor == c.binding.queue_generation
        && (!strict_raw || (RAW_WRITERS.load(SeqCst) == 0 && revision == c.binding.raw))
    {
        return true;
    }
    let f = &w.future;
    current.constructed
        && !current.closed
        && current.mapped
        && f.busy
        && !f.retiring
        && POLLS.load(SeqCst) == 1
        && JOBS.load(SeqCst) == 1
        && RAW_WRITERS.load(SeqCst) == 0
        && cursor_read
        && queue_read
        && cycle_state == f.state
        && f.facts.protects_bound(
            &proof,
            c.binding.source,
            c.binding.generation,
            generation,
            cursor,
            cycle_valid,
        )
}

fn mapped_current(source: u64) -> bool {
    let c = render_copy();
    c.cpu.source == source
        && !c.invalid
        && c.mapped
        && (c.sealed || (c.binding.source != 0 && bound_current_valid(&c, true)))
}

fn abort_pending(reason: u32) {
    let p = pending();
    if p.source == 0 {
        return;
    }
    source_event(RSF_GAME_CPU_SOURCE_ABORT, reason);
    if IDENTITY.get().source == p.source {
        SOURCE_INVALID.set(true);
    }
    PENDING.set(Pending::ZERO);
}

fn invalid_pending() -> u32 {
    let p = pending();
    if p.bound && mapped_current(p.source) {
        return 0;
    }
    if !ENABLED.load(Acquire) {
        return RSF_GAME_CPU_ABORT_QUIESCE;
    }
    if POLLS.load(SeqCst) != 0 || RAW_WRITERS.load(SeqCst) != 0 {
        return RSF_GAME_CPU_ABORT_OVERLAP;
    }
    if POLL_GENERATION.load(SeqCst) != p.generation {
        return RSF_GAME_CPU_ABORT_INTERVENING_POLL;
    }
    let mut current = InputState::ZERO;
    if RAW_REVISION.load(SeqCst) != p.raw || !eligible(&mut current, 0) || current != p.state {
        return RSF_GAME_CPU_ABORT_STATE_CHANGED;
    }
    0
}

fn close_frame() {
    let identity = IDENTITY.get();
    if identity.source == 0 {
        return;
    }
    finish_render_scope();
    let _ = drain_current_abort();
    emit(RSF_GAME_CPU_FRAME_END);
    {
        let mut w = worker();
        if w.current.cpu.source == IDENTITY.get().source {
            w.current.closed = true;
            // No verified Present phase was published for this source. The worker cannot await
            // its receipt once Main closes and joins it.
            if !w.current.sealed && w.future.facts.predecessor == w.current.key {
                w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
    }
    let _ = drain_future();
    IDENTITY.set(CpuIdentity::ZERO);
    RENDERED.set(0);
    SOURCE_INVALID.set(false);
}

// ---------------------------------------------------------------------------------------------
// First-seen refusal receipts. All logging runs outside the worker lock.

/// Claims the first free slot for `key`; false when the key was already recorded or every slot
/// holds another key.
fn first_seen(slots: &[AtomicU64], key: u64, load: Ordering, exchange: Ordering) -> bool {
    for slot in slots {
        let previous = slot.load(load);
        if previous == key {
            return false;
        }
        if previous != 0 {
            continue;
        }
        match slot.compare_exchange(0, key, exchange, load) {
            Ok(_) => return true,
            Err(actual) if actual == key => return false,
            Err(_) => continue,
        }
    }
    false
}

/// First distinct scalar conjunctions, shared hard budget of 32 slots.
fn gate_receipt(
    channel: u32,
    bits: u32,
    expected: u32,
    c: &RenderCycle,
    proof: &WorkerCycle,
    requested: &CpuIdentity,
) {
    let host = host();
    if bits == expected || host.log.is_none() {
        return;
    }
    let key = (u64::from(channel) << 32) | u64::from(bits);
    if !first_seen(&GATE_RECEIPTS, key, SeqCst, SeqCst) {
        return;
    }
    let p = pending();
    log(
        &host,
        format!(
            "Fallout4 CPU native gate={channel} first_missing={} bits=0x{bits:x} expected=0x{expected:x} source={} requested={} identity={} submission={}/{} native={}/{} window={}/{} idle={} depths={}/{}/{}/{} selected={}/{} rendered={} claimed={} invalid={} pending={} tid={}/{} cycle_flags={}{}{}{}{} jobs={} polls={} raw_writers={}.",
            bits.trailing_ones(),
            c.cpu.source,
            requested.source,
            IDENTITY.get().source,
            requested.submission,
            c.cpu.submission,
            c.key.frame,
            proof.frame,
            P(c.key.window),
            P(proof.window),
            c.key.idle,
            IDLE_DEPTH.get(),
            SWAP_DEPTH.get(),
            JOB_DEPTH.get(),
            WRAPPER_DEPTH.get(),
            u32::from(JOB_SELECTED.get()),
            u32::from(WRAPPER_SELECTED.get()),
            RENDERED.get(),
            u32::from(FUTURE_CLAIMED.get()),
            u32::from(SOURCE_INVALID.get()),
            p.source,
            c.tid,
            thread_id(),
            u32::from(c.constructed),
            u32::from(c.sealed),
            u32::from(c.present),
            u32::from(c.closed),
            u32::from(c.invalid),
            JOBS.load(SeqCst),
            POLLS.load(SeqCst),
            RAW_WRITERS.load(SeqCst),
        ),
    );
}

/// Reached and passed downstream checks, with the native wait result and mode when known.
#[derive(Clone, Copy)]
struct PhaseChecks {
    reached: u32,
    passed: u32,
    queue: u32,
    native_result: u32,
    native_mode: u32,
    result_known: bool,
    mode_known: bool,
    generation: u64,
}

impl PhaseChecks {
    fn new() -> Self {
        Self {
            reached: 0,
            passed: 0,
            queue: 0,
            native_result: u32::MAX,
            native_mode: u32::MAX,
            result_known: false,
            mode_known: false,
            generation: 0,
        }
    }

    fn test(&mut self, bit: u32, value: bool) -> bool {
        self.reached |= 1 << bit;
        if value {
            self.passed |= 1 << bit;
        }
        value
    }
}

fn downstream_receipt(
    phase: &str,
    f: &Future,
    proof: &WorkerCycle,
    checks: &PhaseChecks,
    cursor: u32,
) {
    let host = host();
    if !f.facts.active()
        || f.facts.refusal != 0
        || host.log.is_none()
        || DOWNSTREAM_RECEIPTS.fetch_add(1, SeqCst) >= 32
    {
        return;
    }
    let p = &f.facts.predecessor;
    log(
        &host,
        format!(
            "Fallout4 CPU downstream candidate/refusal phase={phase} source={} ticket_phase={} predecessor={} proof_source={} native={}/{} idle={}/{} tls_idle={} generation={}/{} cursor={}/{cursor} queue={} reached=0x{:x} passed=0x{:x} busy={} retiring={} main={}/{} window={}/{} worker={}/{} manager={}/{} queue_owner={}/{} receiver={}/{} events={},{}/{},{} session={}/{} wait_result_known={} wait_result=0x{:x} mode_known={} mode={} callback_inflight={} callback_returned={} native_poll_started={} reserve_announced={}.",
            f.facts.cpu.source,
            f.facts.phase as u32,
            p.source,
            proof.source,
            p.frame,
            proof.frame,
            p.idle,
            proof.idle,
            IDLE_SERIAL.get(),
            f.facts.generation,
            checks.generation,
            f.facts.cursor,
            checks.queue,
            checks.reached,
            checks.passed,
            u32::from(f.busy),
            u32::from(f.retiring),
            P(p.main),
            P(proof.main),
            P(p.window),
            P(proof.window),
            P(p.worker),
            P(proof.worker),
            P(p.manager),
            P(proof.manager),
            P(p.queue),
            P(proof.queue),
            P(p.receiver),
            P(proof.receiver),
            P(p.start_event),
            P(p.complete_event),
            P(proof.start_event),
            P(proof.complete_event),
            p.session,
            proof.session,
            u32::from(checks.result_known),
            checks.native_result,
            u32::from(checks.mode_known),
            checks.native_mode,
            u32::from(f.facts.callback_inflight),
            u32::from(f.facts.callback_returned),
            u32::from(f.facts.native_poll_started),
            u32::from(f.reserve_announced),
        ),
    );
}

// ---------------------------------------------------------------------------------------------
// The worker poll and its future source.

/// Reserves the future source for the selected worker poll. True when this poll owns a mapped
/// ticket, even when a later proof refused it.
fn reserve_worker_poll(
    manager: usize,
    caller: usize,
    generation: &mut u64,
    previous_polls: &mut u32,
) -> bool {
    let c = render_copy();
    let mut proof = c.key;
    let mut native = InputState::ZERO;
    let tid = thread_id();
    let worker_call = caller == image() + native::WRAPPER_POLL_CALLER;
    let native_valid_before =
        worker_call && native_cycle(&mut proof, &mut native, false, CyclePhase::ReserveEntry);
    let (mut queued, mut consumed) = (0u32, 0u32);
    let binding_unchanged = c.binding.source == 0
        || (c.binding.bound
            && c.binding.source == c.cpu.source
            && c.binding.state == native
            && load(&mut queued, native.queue + native::QUEUE_SEQUENCE)
            && load(&mut consumed, native.receiver + native::RECEIVER_CURSOR)
            && consumed == c.binding.queue_generation);
    let frequency = if native_valid_before {
        qpc_frequency()
    } else {
        0
    };
    let host = host();
    let mut copy = Future::ZERO;
    let mut bits = 0;
    let mut registered = false;
    {
        let mut guard = worker();
        let w = &mut *guard;
        // Publish the one qualified no-native-poll-yet intention with its attempt counters. Main
        // cannot observe an advanced counter without this ticket.
        *previous_polls = POLLS.fetch_add(1, SeqCst);
        *generation = POLL_GENERATION.fetch_add(1, SeqCst) + 1;
        let current = &w.current;
        let source_exists = current.cpu.source != 0
            && current.cpu.submission != 0
            && current.cpu.session == host.session_id
            && current.key.source == current.cpu.source;
        let binding_ready = binding_unchanged
            && (c.binding.source == 0
                || (*generation > c.binding.generation && *generation - c.binding.generation == 1));
        bits = predicate_bits(&[
            enabled(),
            JOB_SELECTED.get(),
            WRAPPER_SELECTED.get(),
            JOB_DEPTH.get() == 1,
            WRAPPER_DEPTH.get() == 1,
            JOBS.load(SeqCst) == 1,
            *previous_polls == 0,
            RAW_WRITERS.load(SeqCst) == 0,
            current.constructed,
            source_exists,
            binding_ready,
            !current.closed,
            !current.invalid,
            c.tid != tid,
            native_valid_before,
            proof == current.key,
            native == current.state,
            native.manager == manager,
        ]);
        if worker_call && pending().source == 0 && bits == 0x3ffff && !w.future.facts.active() {
            let next = CpuIdentity {
                session: host.session_id,
                source: NEXT_SOURCE.fetch_add(1, SeqCst),
                submission: 0,
                began_qpc: 0,
                frequency,
            };
            let bound_generation = if c.binding.source != 0 {
                c.binding.generation
            } else {
                0
            };
            let f = &mut w.future;
            if f.facts.reserve(&c.key, &next, *generation)
                && f.facts
                    .begin_callback(bound_generation, c.binding.queue_generation)
            {
                f.state = native;
                f.busy = true;
                copy = *f;
                w.current.mapped = true;
                WORKER_SOURCE.set(next.source);
                registered = true;
            } else {
                // This local candidate has not announced RESERVE or acquired any host
                // obligation. Never clear an older or published ticket here.
                *f = Future::ZERO;
            }
        }
    }
    if !registered {
        if worker_call {
            gate_receipt(1, bits, 0x3ffff, &c, &proof, &CpuIdentity::ZERO);
        }
        return false;
    }
    let mut announce = false;
    {
        let mut w = worker();
        let current_matches = w.current.key == copy.facts.predecessor && !w.current.invalid;
        let f = &mut w.future;
        if f.facts.cpu.source == copy.facts.cpu.source
            && f.facts.valid()
            && enabled()
            && current_matches
        {
            f.reserve_announced = true;
            announce = true;
        } else {
            f.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    let sent = announce && source_fact(&ticket_pending(&copy), RSF_GAME_CPU_SOURCE_RESERVE, 0);
    // The raw snapshot belongs immediately after the synchronous callback and before the
    // original Poll. Callback return is not a core acceptance receipt.
    let revision = RAW_REVISION.load(SeqCst);
    let after = {
        let mut w = worker();
        if w.future.facts.cpu.source == copy.facts.cpu.source {
            w.future.facts.end_callback();
        }
        w.current
    };
    proof = copy.facts.predecessor;
    let native_valid = sent
        && enabled()
        && native_cycle(
            &mut proof,
            &mut native,
            after.joining,
            CyclePhase::ReserveReturn,
        )
        && proof == copy.facts.predecessor
        && native == copy.state
        && RAW_WRITERS.load(SeqCst) == 0
        && RAW_REVISION.load(SeqCst) == revision
        && POLLS.load(SeqCst) == 1
        && POLL_GENERATION.load(SeqCst) == *generation;
    let (mut armed, mut unsafe_bound) = (false, false);
    {
        let mut w = worker();
        if w.future.facts.cpu.source == copy.facts.cpu.source {
            let current = w.current;
            let predecessor_safe =
                WorkerTicket::poll_safe(current.binding.source, current.closed, current.sealed);
            unsafe_bound = current.key == copy.facts.predecessor && !predecessor_safe;
            if native_valid
                && current.key == copy.facts.predecessor
                && !current.invalid
                && w.future
                    .facts
                    .arm(&proof, *generation, revision, predecessor_safe)
            {
                armed = true;
            } else {
                w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
    }
    if unsafe_bound {
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, true, None);
    }
    if !armed {
        let mask = predicate_bits(&[sent, native_valid, !unsafe_bound, armed]);
        gate_receipt(3, mask, 0xf, &after, &proof, &copy.facts.cpu);
    }
    true
}

fn sample_worker_poll(generation: u64, began: u64) {
    let f = future_copy();
    let worker_source = WORKER_SOURCE.get();
    if worker_source == 0 || f.facts.cpu.source != worker_source {
        return;
    }
    let c = render_copy();
    let mut proof = f.facts.predecessor;
    let mut native = InputState::ZERO;
    let mut cursor = 0u32;
    let valid = enabled()
        && native_cycle(&mut proof, &mut native, c.joining, CyclePhase::Sampled)
        && proof == f.facts.predecessor
        && native == f.state
        && POLLS.load(SeqCst) == 1
        && RAW_WRITERS.load(SeqCst) == 0
        && load(&mut cursor, native.queue + native::QUEUE_SEQUENCE);
    let mut sampled = None;
    {
        let mut w = worker();
        if w.future.facts.cpu.source != worker_source {
            return;
        }
        let current_matches = w.current.key == f.facts.predecessor && !w.current.invalid;
        let revision = RAW_REVISION.load(SeqCst);
        let unchanged = POLL_GENERATION.load(SeqCst) == generation;
        if valid
            && current_matches
            && w.future
                .facts
                .sample(generation, revision, cursor, began, unchanged)
        {
            sampled = Some(w.future);
        } else {
            w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    if let Some(sampled) = sampled
        && !source_fact(&ticket_pending(&sampled), RSF_GAME_CPU_SOURCE_SAMPLED, 0)
    {
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, false, None);
    }
}

fn worker_consumer_proof(
    f: &Future,
    proof: &mut WorkerCycle,
    cursor: &mut u32,
    before_bind: bool,
    trace: &mut PhaseChecks,
    phase: CyclePhase,
) -> bool {
    let c = render_copy();
    let mut native = InputState::ZERO;
    *proof = f.facts.predecessor;
    if !trace.test(0, enabled()) {
        return false;
    }
    let cycle = native_cycle(proof, &mut native, c.joining, phase);
    if !trace.test(1, cycle)
        || !trace.test(2, *proof == f.facts.predecessor)
        || !trace.test(3, native == f.state)
        || !trace.test(4, POLLS.load(SeqCst) == 0)
    {
        return false;
    }
    trace.generation = POLL_GENERATION.load(SeqCst);
    if !trace.test(5, trace.generation == f.facts.generation) {
        return false;
    }
    let raw_unchanged =
        !before_bind || (RAW_WRITERS.load(SeqCst) == 0 && RAW_REVISION.load(SeqCst) == f.facts.raw);
    if !trace.test(6, raw_unchanged) {
        return false;
    }
    let mut queue = trace.queue;
    let queue_read = load(&mut queue, native.queue + native::QUEUE_SEQUENCE);
    trace.queue = queue;
    if !trace.test(7, queue_read) || !trace.test(8, !before_bind || trace.queue == f.facts.cursor) {
        return false;
    }
    let cursor_read = load(cursor, native.receiver + native::RECEIVER_CURSOR);
    trace.test(9, cursor_read) && trace.test(10, *cursor == f.facts.cursor)
}

fn begin_worker_consumer(receiver: usize, caller: usize) -> bool {
    let f = future_copy();
    if !f.facts.active() {
        let c = render_copy();
        if c.binding.source != 0 && !c.closed {
            invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, true, None);
        }
        return false;
    }
    let delivery = DELIVERY.get();
    let selected = JOB_SELECTED.get()
        && WRAPPER_SELECTED.get()
        && WORKER_SOURCE.get() == f.facts.cpu.source
        && delivery.queue == f.facts.predecessor.queue
        && delivery.receiver == receiver
        && caller == image() + native::PLAYER_CONTROLS_CALLER;
    let mut proof = WorkerCycle::ZERO;
    let mut cursor = 0u32;
    let now = qpc();
    let valid = selected
        && worker_consumer_proof(
            &f,
            &mut proof,
            &mut cursor,
            true,
            &mut PhaseChecks::new(),
            CyclePhase::ConsumerBegin,
        );
    let mut begun = Future::ZERO;
    {
        let mut w = worker();
        if w.future.facts.cpu.source != f.facts.cpu.source {
            return false;
        }
        let current_matches = w.current.key == f.facts.predecessor && !w.current.invalid;
        let generation = POLL_GENERATION.load(SeqCst);
        let revision = RAW_REVISION.load(SeqCst);
        if !valid || !current_matches || !w.future.facts.begin(now, generation, revision, cursor) {
            w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        } else {
            begun = w.future;
        }
    }
    if !begun.facts.frame_begun {
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, true, None);
        return false;
    }
    let frame_sent = emit_identity(&begun.facts.cpu, RSF_GAME_CPU_FRAME_BEGIN, 0);
    let check = future_copy();
    if !frame_sent
        || !worker_consumer_proof(
            &check,
            &mut proof,
            &mut cursor,
            true,
            &mut PhaseChecks::new(),
            CyclePhase::FrameReturn,
        )
    {
        invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, false, None);
        return false;
    }
    let bound_sent = source_fact(&ticket_pending(&check), RSF_GAME_CPU_SOURCE_BIND, 0);
    let mut trace = PhaseChecks::new();
    let still_valid = trace.test(11, bound_sent)
        && worker_consumer_proof(
            &check,
            &mut proof,
            &mut cursor,
            true,
            &mut trace,
            CyclePhase::BindReturn,
        );
    let mut refused = false;
    {
        let mut w = worker();
        if w.future.facts.cpu.source == begun.facts.cpu.source {
            let accepted = still_valid
                && trace.test(12, w.current.key == begun.facts.predecessor)
                && trace.test(13, !w.current.invalid)
                && {
                    trace.generation = POLL_GENERATION.load(SeqCst);
                    let revision = RAW_REVISION.load(SeqCst);
                    let bound = w.future.facts.bind(trace.generation, revision, cursor);
                    trace.test(14, bound)
                };
            if !accepted {
                refused = w.future.facts.refusal == 0;
                w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
    }
    if refused {
        downstream_receipt("bind-return", &check, &proof, &trace, cursor);
    }
    true
}

fn end_worker_consumer() {
    let f = future_copy();
    let mut proof = WorkerCycle::ZERO;
    let mut cursor = 0u32;
    let worker_source = WORKER_SOURCE.get();
    if worker_source == 0 || f.facts.cpu.source != worker_source {
        return;
    }
    let mut trace = PhaseChecks::new();
    let valid = worker_consumer_proof(
        &f,
        &mut proof,
        &mut cursor,
        false,
        &mut trace,
        CyclePhase::ConsumerReturn,
    );
    let mut refused = false;
    {
        let mut w = worker();
        if w.future.facts.cpu.source == worker_source {
            let accepted = valid && {
                trace.generation = POLL_GENERATION.load(SeqCst);
                let consumed = w.future.facts.consumed(trace.generation, cursor);
                trace.test(11, consumed)
            };
            if !accepted {
                refused = w.future.facts.refusal == 0;
                w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
    }
    if refused {
        downstream_receipt("consumer-return", &f, &proof, &trace, cursor);
    }
}

// ---------------------------------------------------------------------------------------------
// Detour entry and exit decisions.
//
// cpu_events.cpp keeps every native shell. A shell captures its return address and the thread's
// last error, calls the entry decision here, restores that error, calls the original through its
// typed trampoline on its own C++ frame, captures the native last error, calls the exit decision
// here and restores the native error. No Rust frame is on the stack during an engine call, so a
// native unwind passes only through the C++ shells. Their destructors retire the entry without
// observing a successful native return.
//
// The scope is the shell's stack receipt for one call. The entry writes it with its hook tag;
// the exit consumes it and refuses a receipt of another hook.

const SCOPE_TAG: u64 = 0x5253_4634_4350_0000;

/// Records `value` as this call's receipt for `hook`.
fn open_scope<T: Copy>(scope: *mut rsf_fallout4_cpu_scope, hook: u32, value: T) {
    const {
        assert!(
            size_of::<T>() <= size_of::<[u64; RSF_FALLOUT4_CPU_SCOPE_WORDS as usize]>()
                && align_of::<T>() <= align_of::<u64>()
        )
    };
    // SAFETY: the shell passes its own stack receipt, or null.
    let Some(scope) = (unsafe { scope.as_mut() }) else {
        return;
    };
    // SAFETY: the state words hold T by the assertion above.
    unsafe { scope.state.as_mut_ptr().cast::<T>().write(value) };
    scope.tag = SCOPE_TAG | u64::from(hook);
}

/// This call's receipt for `hook`. `consume` closes it so a second exit refuses.
fn scope_value<T: Copy>(scope: *mut rsf_fallout4_cpu_scope, hook: u32, consume: bool) -> Option<T> {
    // SAFETY: the shell passes its own stack receipt, or null.
    let scope = unsafe { scope.as_mut() }?;
    if scope.tag != SCOPE_TAG | u64::from(hook) {
        return None;
    }
    if consume {
        scope.tag = 0;
    }
    // SAFETY: open_scope wrote a T for this hook tag.
    Some(unsafe { scope.state.as_ptr().cast::<T>().read() })
}

fn enter() {
    ENTERED.fetch_add(1, AcqRel);
}

fn leave() {
    ENTERED.fetch_sub(1, AcqRel);
}

/// PeekMessageA entry. Returns the immutable continuation the shell calls.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_peek_enter() -> *mut c_void {
    enter();
    ORIGINAL_PEEK.load(Acquire) as *mut c_void
}

/// PeekMessageA exit with the native result and the engine's own arguments.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_peek_exit(
    caller: usize,
    result: i32,
    message: *mut c_void,
    window: *mut c_void,
    minimum: u32,
    maximum: u32,
    flags: u32,
) {
    let base = image();
    if ENABLED.load(Acquire)
        && result != 0
        && !message.is_null()
        && window.is_null()
        && minimum == 0
        && maximum == 0
        && flags == PM_REMOVE
        && native::PEEK_CALLERS
            .iter()
            .any(|&site| caller == base + site)
        && let Some(actual) = read::<MSG>(message as usize)
    {
        received(0, actual.message, true, actual.hwnd as usize as u64);
    }
    leave();
}

/// Native RAWINPUT handler entry, not the later buffered Poll methods.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_raw_enter(hook: u32, caller: usize) {
    enter();
    RAW_WRITERS.fetch_add(1, SeqCst);
    RAW_REVISION.fetch_add(1, SeqCst);
    let selected = match hook {
        RSF_FALLOUT4_CPU_HOOK_KEYBOARD => Some((native::KEYBOARD_CALLER, RSF_GAME_INPUT_KEYBOARD)),
        RSF_FALLOUT4_CPU_HOOK_MOUSE => Some((native::MOUSE_CALLER, RSF_GAME_INPUT_MOUSE)),
        _ => None,
    };
    if let Some((expected_caller, kind)) = selected
        && ENABLED.load(Acquire)
        && caller == image() + expected_caller
    {
        received(kind, 0, false, 0);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_raw_exit() {
    RAW_REVISION.fetch_add(1, SeqCst);
    RAW_WRITERS.fetch_sub(1, SeqCst);
    leave();
}

#[derive(Clone, Copy)]
struct IdleScope {
    owns: bool,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_idle_enter(
    caller: usize,
    main: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    IDLE_DEPTH.set(IDLE_DEPTH.get() + 1);
    let main = main as usize;
    let owns = ENABLED.load(Acquire)
        && IDLE_DEPTH.get() == 1
        && caller == image() + native::IDLE_CALLER
        && IDLE_MAIN.get() == 0;
    if owns {
        IDLE_MAIN.set(main);
        IDLE_SERIAL.set(IDLE_SERIAL.get() + 1);
        let f = future_copy();
        if f.facts.active()
            && (f.facts.predecessor.main != main
                || f.facts.predecessor.idle.wrapping_add(1) < IDLE_SERIAL.get())
        {
            invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED, false, None);
        }
    }
    open_scope(scope, RSF_FALLOUT4_CPU_HOOK_IDLE, IdleScope { owns });
}

/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_idle_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(IdleScope { owns }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_IDLE, true) else {
        return;
    };
    if owns {
        abort_pending(if !enabled() {
            RSF_GAME_CPU_ABORT_QUIESCE
        } else if pending().bound {
            RSF_GAME_CPU_ABORT_STATE_CHANGED
        } else {
            RSF_GAME_CPU_ABORT_NOT_CONSUMED
        });
        close_frame();
        let f = future_copy();
        if f.facts.active() && f.facts.predecessor.idle < IDLE_SERIAL.get() {
            invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED, false, None);
        }
        IDLE_MAIN.set(0);
    }
    IDLE_DEPTH.set(IDLE_DEPTH.get() - 1);
    leave();
}

/// The poll facts the candidate receipt reports.
#[derive(Clone, Copy)]
struct PollReceipt {
    caller: usize,
    candidate: bool,
    state_read: StateRead,
    before: InputState,
    generation: u64,
    depth: u32,
    previous_polls: u32,
    source: u64,
    raw_writers: u32,
    idle_main: usize,
    manager: usize,
    lifecycle: bool,
}

#[derive(Clone, Copy)]
struct PollScope {
    receipt: PollReceipt,
    mapped: bool,
}

/// A fixed process-wide fact set, not a source or event ledger. Startup repeats use one slot
/// instead of exhausting the budget before world transitions.
fn poll_receipt(r: &PollReceipt) {
    let host = host();
    if host.log.is_none() {
        return;
    }
    let base = image();
    let caller_class: u32 = if r.caller == base + native::MAIN_POLL_CALLER {
        0
    } else if r.caller == base + native::WRAPPER_POLL_CALLER {
        1
    } else if r.caller == base + native::WORKER_POLL_CALLER {
        2
    } else {
        // Unknown native callers are forwarded, not classified.
        return;
    };
    let flags = r
        .before
        .flags
        .iter()
        .enumerate()
        .fold(0u32, |bits, (index, &flag)| {
            bits | (u32::from(flag != 0) << index)
        });
    let predicates = predicate_bits(&[
        r.lifecycle,
        r.depth == 1,
        r.previous_polls == 0,
        r.source == 0,
        r.raw_writers == 0,
        r.before.manager == r.manager,
        r.candidate,
        r.idle_main != 0,
        r.before.main == r.idle_main && r.before.main != 0,
    ]);
    let key = 1u64
        | (u64::from(caller_class) << 1)
        | (u64::from(predicates) << 3)
        | ((r.state_read as u64) << 12)
        | (u64::from(flags & 0xef) << 17);
    if !first_seen(&POLL_COMBINATIONS, key, Acquire, AcqRel) {
        return;
    }
    const CALLERS: [&str; 3] = ["main", "wrapper", "worker"];
    log(
        &host,
        format!(
            "Fallout4 native poll candidate={} class={} caller={} predicate_bits=0x{predicates:x} state={} partial=1 generation={} idle_depth={} overlap={} source={} raw_writers={} idle_main={} native_main={} manager={}/{} polling_enabled={} main_flags_nonzero=0x{flags:x} menu={} queue={} receiver={}.",
            u32::from(r.candidate),
            CALLERS[caller_class as usize],
            P(r.caller),
            native::STATE_NAMES[r.state_read as usize],
            r.generation,
            r.depth,
            r.previous_polls,
            r.source,
            r.raw_writers,
            P(r.idle_main),
            P(r.before.main),
            P(r.manager),
            P(r.before.manager),
            r.before.polling_enabled,
            r.before.menu,
            P(r.before.queue),
            P(r.before.receiver),
        ),
    );
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_poll_enter(
    caller: usize,
    manager: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let manager_address = manager as usize;
    let (mut previous_polls, mut generation) = (0u32, 0u64);
    let mapped = reserve_worker_poll(
        manager_address,
        caller,
        &mut generation,
        &mut previous_polls,
    );
    let refusal = if previous_polls != 0 {
        RSF_GAME_CPU_ABORT_OVERLAP
    } else {
        RSF_GAME_CPU_ABORT_INTERVENING_POLL
    };
    if pending().source != 0 {
        abort_pending(refusal);
    }
    let mut before = InputState::ZERO;
    let mut state_read = StateRead::NotCalled;
    if !mapped {
        let current = render_copy();
        let mut proof = current.key;
        let mut native = InputState::ZERO;
        let poll_only = enabled()
            && previous_polls == 0
            && RAW_WRITERS.load(SeqCst) == 0
            && native_cycle(&mut proof, &mut native, false, CyclePhase::Other)
            && proof == current.key
            && native == current.state;
        invalidate_worker(refusal, true, poll_only.then_some(&current.key));
    }
    let lifecycle = ENABLED.load(Acquire);
    let depth_at_poll = IDLE_DEPTH.get();
    let source_at_poll = IDENTITY.get().source;
    let idle_at_poll = IDLE_MAIN.get();
    let raw_at_poll = RAW_WRITERS.load(SeqCst);
    let candidate = lifecycle
        && caller == image() + native::MAIN_POLL_CALLER
        && depth_at_poll == 1
        && previous_polls == 0
        && source_at_poll == 0
        && raw_at_poll == 0
        && {
            state_read = snapshot(&mut before, 0);
            state_read == StateRead::Eligible
        }
        && before.manager == manager_address;
    if candidate {
        let frequency = qpc_frequency();
        let source = NEXT_SOURCE.fetch_add(1, SeqCst);
        let raw = RAW_REVISION.load(SeqCst);
        update_pending(|p| {
            p.source = source;
            p.generation = generation;
            p.frequency = frequency;
            p.raw = raw;
            p.state = before;
        });
        source_event(RSF_GAME_CPU_SOURCE_RESERVE, 0);
    }
    let worker_source = WORKER_SOURCE.get();
    if mapped {
        let mut unsafe_poll = false;
        {
            let mut w = worker();
            if w.future.facts.cpu.source == worker_source {
                let c = w.current;
                unsafe_poll = !enabled()
                    || c.key != w.future.facts.predecessor
                    || c.invalid
                    || POLLS.load(SeqCst) != 1
                    || POLL_GENERATION.load(SeqCst) != generation
                    || RAW_WRITERS.load(SeqCst) != 0
                    || RAW_REVISION.load(SeqCst) != w.future.facts.raw
                    || !WorkerTicket::poll_safe(c.binding.source, c.closed, c.sealed);
                if unsafe_poll {
                    w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
        }
        if unsafe_poll {
            invalidate_worker(RSF_GAME_CPU_ABORT_STATE_CHANGED, true, None);
        }
        let mut w = worker();
        if w.future.facts.cpu.source == worker_source {
            w.future.facts.native_poll_entry();
        }
    }
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_POLL,
        PollScope {
            receipt: PollReceipt {
                caller,
                candidate,
                state_read,
                before,
                generation,
                depth: depth_at_poll,
                previous_polls,
                source: source_at_poll,
                raw_writers: raw_at_poll,
                idle_main: idle_at_poll,
                manager: manager_address,
                lifecycle,
            },
            mapped,
        },
    );
}

/// `sample_begin` is the QPC the shell took after restoring the last error and immediately
/// before the native Poll: RESERVE has returned before it.
///
/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_poll_exit(
    scope: *mut rsf_fallout4_cpu_scope,
    sample_begin: u64,
) {
    let Some(PollScope { receipt, mapped }) =
        scope_value::<PollScope>(scope, RSF_FALLOUT4_CPU_HOOK_POLL, true)
    else {
        return;
    };
    let (candidate, generation, before) = (receipt.candidate, receipt.generation, receipt.before);
    let p = pending();
    if candidate && p.source != 0 && p.generation == generation {
        let mut after = InputState::ZERO;
        if POLLS.load(SeqCst) != 1
            || POLL_GENERATION.load(SeqCst) != generation
            || RAW_WRITERS.load(SeqCst) != 0
        {
            abort_pending(RSF_GAME_CPU_ABORT_OVERLAP);
        } else if !enabled() {
            abort_pending(RSF_GAME_CPU_ABORT_QUIESCE);
        } else {
            let mut queue_generation = p.queue_generation;
            let unchanged = RAW_REVISION.load(SeqCst) == p.raw
                && eligible(&mut after, 0)
                && before == after
                && load(&mut queue_generation, before.queue + native::QUEUE_SEQUENCE);
            update_pending(|p| p.queue_generation = queue_generation);
            if unchanged {
                update_pending(|p| {
                    p.sample_begin = sample_begin;
                    p.sampled = true;
                });
                source_event(RSF_GAME_CPU_SOURCE_SAMPLED, 0);
            } else {
                abort_pending(RSF_GAME_CPU_ABORT_STATE_CHANGED);
            }
        }
    }
    if mapped {
        sample_worker_poll(generation, sample_begin);
    }
    poll_receipt(&receipt);
    POLLS.fetch_sub(1, SeqCst);
    leave();
}

/// The queue's enqueue sequence still equals the sampled one.
fn sampled_sequence(p: &Pending) -> bool {
    let mut queue_generation = 0u32;
    load(
        &mut queue_generation,
        p.state.queue + native::QUEUE_SEQUENCE,
    ) && queue_generation == p.queue_generation
}

#[derive(Clone, Copy)]
struct PlayerControlsScope {
    worker_consuming: bool,
    main_source: u64,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_player_controls_enter(
    caller: usize,
    receiver: *mut c_void,
    events: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let receiver_address = receiver as usize;
    let selected_caller = caller == image() + native::PLAYER_CONTROLS_CALLER;
    // The selected native dispatcher still holds its outer queue read lock here. Report delivery
    // before source adoption; this is independent of pending polls.
    let delivery = DELIVERY.get();
    if delivery.queue != 0
        && delivery.receiver == receiver_address
        && selected_caller
        && native::delivery_current(ENABLED.load(Acquire), delivery.queue, receiver_address)
        && native::controller_delivered(events as usize)
    {
        received(RSF_GAME_INPUT_CONTROLLER, 0, false, 0);
    }
    let worker_consuming = begin_worker_consumer(receiver_address, caller);
    let p = pending();
    if CONSUMER_ADMITTED.get()
        && p.source != 0
        && !p.bound
        && IDLE_DEPTH.get() == 1
        && selected_caller
    {
        let invalid = invalid_pending();
        if invalid != 0
            || !p.sampled
            || IDENTITY.get().source != 0
            || receiver_address != p.state.receiver
            || !sampled_sequence(&p)
        {
            abort_pending(if invalid != 0 {
                invalid
            } else {
                RSF_GAME_CPU_ABORT_STATE_CHANGED
            });
        } else {
            IDENTITY.set(CpuIdentity {
                session: host().session_id,
                source: p.source,
                submission: 0,
                began_qpc: qpc(),
                frequency: p.frequency,
            });
            RENDERED.set(0);
            emit(RSF_GAME_CPU_FRAME_BEGIN);
            // Recheck after the host callback before asserting native consumption.
            let mut changed = invalid_pending();
            if changed == 0 && !sampled_sequence(&pending()) {
                changed = RSF_GAME_CPU_ABORT_STATE_CHANGED;
            }
            if changed != 0 {
                abort_pending(changed);
                close_frame();
            } else {
                source_event(RSF_GAME_CPU_SOURCE_BIND, 0);
                update_pending(|p| p.bound = true);
                let mut changed_after_bind = invalid_pending();
                if changed_after_bind == 0 && !sampled_sequence(&pending()) {
                    changed_after_bind = RSF_GAME_CPU_ABORT_STATE_CHANGED;
                }
                if changed_after_bind != 0 {
                    abort_pending(changed_after_bind);
                    close_frame();
                }
            }
        }
    }
    let mut main_source = 0;
    // The verified native PlayerControls callback begins consumption-driven simulation. Report
    // this actual boundary, never the earlier BIND callback.
    if worker_consuming {
        let observed = future_copy();
        if observed.facts.valid()
            && observed.facts.phase == Phase::Bound
            && observed.facts.cpu.source == WORKER_SOURCE.get()
        {
            let sent = emit_identity(&observed.facts.cpu, RSF_GAME_CPU_SIMULATION_BEGIN, 0);
            let mut w = worker();
            if w.future.facts.cpu.source == observed.facts.cpu.source {
                w.future.simulation_started = sent;
            }
        }
    } else {
        let p = pending();
        let identity = IDENTITY.get();
        if CONSUMER_ADMITTED.get() && p.bound && p.source == identity.source && selected_caller {
            main_source = p.source;
            let started = emit_identity(&identity, RSF_GAME_CPU_SIMULATION_BEGIN, 0);
            update_pending(|p| p.simulation_started = started);
        }
    }
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS,
        PlayerControlsScope {
            worker_consuming,
            main_source,
        },
    );
}

/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_player_controls_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(PlayerControlsScope {
        worker_consuming, ..
    }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS, true)
    else {
        return;
    };
    if worker_consuming {
        end_worker_consumer();
    }
    leave();
}

#[derive(Clone, Copy)]
struct ConsumeScope {
    parent: bool,
    parent_delivery: Delivery,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_consume_enter(
    caller: usize,
    queue: *mut c_void,
    receiver: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let (queue_address, receiver_address) = (queue as usize, receiver as usize);
    let parent = CONSUMER_ADMITTED.get();
    let parent_delivery = DELIVERY.get();
    let base = image();
    let delivering = (caller == base + native::CONSUME_CALLER
        || (WRAPPER_SELECTED.get() && caller == base + native::WORKER_CONSUME_CALLER))
        && native::delivery_current(ENABLED.load(Acquire), queue_address, receiver_address);
    DELIVERY.set(if delivering {
        Delivery {
            queue: queue_address,
            receiver: receiver_address,
        }
    } else {
        NO_DELIVERY
    });
    let p = pending();
    CONSUMER_ADMITTED.set(
        p.source != 0
            && !p.bound
            && IDLE_DEPTH.get() == 1
            && caller == base + native::CONSUME_CALLER
            && queue_address == p.state.queue
            && receiver_address == p.state.receiver,
    );
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_CONSUME,
        ConsumeScope {
            parent,
            parent_delivery,
        },
    );
}

/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_consume_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(ConsumeScope {
        parent,
        parent_delivery,
    }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_CONSUME, true)
    else {
        return;
    };
    CONSUMER_ADMITTED.set(parent);
    DELIVERY.set(parent_delivery);
    leave();
}

#[derive(Clone, Copy)]
struct SwapScope {
    owns: bool,
    parent_admitted: bool,
}

/// Swap entry. Every path, including an unowned Swap, enters the scope that the shell's
/// destructor closes with rsf_fallout4_cpu_swap_leave.
///
/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_swap_enter(
    caller: usize,
    main: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    SWAP_DEPTH.set(SWAP_DEPTH.get() + 1);
    let parent_admitted = SWAP_ADMITTED.get();
    SWAP_ADMITTED.set(false);
    let main = main as usize;
    let owns = ENABLED.load(Acquire)
        && caller == image() + native::SWAP_CALLER
        && SWAP_DEPTH.get() == 1
        && IDLE_DEPTH.get() <= 1;
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_SWAP,
        SwapScope {
            owns,
            parent_admitted,
        },
    );
    if !owns {
        abort_pending(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        return;
    }
    SWAP_ADMITTED.set(true);
    let p = pending();
    if p.source != 0 {
        let invalid = invalid_pending();
        if invalid != 0 || !p.bound || IDLE_MAIN.get() != main {
            abort_pending(if invalid != 0 {
                invalid
            } else {
                RSF_GAME_CPU_ABORT_NOT_CONSUMED
            });
            close_frame();
        }
    }
    if SOURCE_INVALID.get() {
        close_frame();
    }
    if IDENTITY.get().source == 0 {
        claim_future(main);
    }
    if IDENTITY.get().source == 0 && !future_copy().facts.active() {
        let frequency = qpc_frequency();
        let source = NEXT_SOURCE.fetch_add(1, SeqCst);
        IDENTITY.set(CpuIdentity {
            session: host().session_id,
            source,
            submission: 0,
            began_qpc: qpc(),
            frequency,
        });
        RENDERED.set(0);
        emit(RSF_GAME_CPU_FRAME_BEGIN);
    }
    // This exact C346AC call leaves native update work and enters Swap's rendering path. Close
    // only the sample actually consumed for this source.
    let simulation = future_copy();
    let identity = IDENTITY.get();
    let p = pending();
    if (FUTURE_CLAIMED.get()
        && simulation.facts.valid()
        && simulation.facts.cpu.source == identity.source
        && simulation.simulation_started)
        || (p.bound && p.source == identity.source && p.simulation_started)
    {
        let _ = emit_identity(&identity, RSF_GAME_CPU_SIMULATION_END, 0);
    }
}

/// Swap exit after the native Swap returned. The scope stays open for the leave.
///
/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_swap_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(SwapScope { owns: true, .. }) =
        scope_value::<SwapScope>(scope, RSF_FALLOUT4_CPU_HOOK_SWAP, false)
    else {
        return;
    };
    if pending().source != 0 {
        let invalid = invalid_pending();
        if invalid != 0 {
            abort_pending(invalid);
        }
    }
    if FUTURE_CLAIMED.get() {
        invalidate_worker(RSF_GAME_CPU_ABORT_NOT_CONSUMED, false, None);
        IDENTITY.set(CpuIdentity::ZERO);
        RENDERED.set(0);
        FUTURE_CLAIMED.set(false);
    } else {
        close_frame();
    }
    {
        let mut w = worker();
        if w.current.cpu.source != 0 && w.current.key.idle == IDLE_SERIAL.get() {
            w.current.swapped = true;
        }
    }
    PENDING.set(Pending::ZERO);
}

/// The Swap scope close, run by the shell's destructor on return and on a native unwind, as the
/// previous native Exit guard was.
///
/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_swap_leave(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(SwapScope {
        parent_admitted, ..
    }) = scope_value::<SwapScope>(scope, RSF_FALLOUT4_CPU_HOOK_SWAP, true)
    else {
        return;
    };
    SWAP_ADMITTED.set(parent_admitted);
    SWAP_DEPTH.set(SWAP_DEPTH.get() - 1);
    leave();
}

/// The next Swap claims the joined worker successor without a second FRAME_BEGIN.
fn claim_future(main: usize) {
    let f = future_copy();
    let mut proof = f.facts.predecessor;
    let mut native = InputState::ZERO;
    let (mut cursor, mut sequence) = (0u32, 0u32);
    proof.source = f.facts.cpu.source;
    proof.idle = IDLE_SERIAL.get();
    let mut trace = PhaseChecks::new();
    let valid = trace.test(0, f.facts.active())
        && trace.test(1, enabled())
        && trace.test(
            2,
            native_cycle(&mut proof, &mut native, false, CyclePhase::SwapClaim),
        )
        && trace.test(3, proof.main == main)
        && trace.test(4, native == f.state)
        && trace.test(
            5,
            load(&mut cursor, native.receiver + native::RECEIVER_CURSOR),
        )
        && trace.test(
            6,
            load(&mut sequence, native.queue + native::QUEUE_SEQUENCE),
        )
        && trace.test(7, cursor == f.facts.cursor)
        && trace.test(8, POLLS.load(SeqCst) == 0);
    trace.queue = sequence;
    let mut claimed = false;
    let mut refused = false;
    if f.facts.active() {
        let mut w = worker();
        let future = &mut w.future;
        let accepted = trace.test(9, !future.busy)
            && trace.test(10, !future.retiring)
            && trace.test(11, future.facts.cpu.source == f.facts.cpu.source)
            && valid
            && {
                trace.generation = POLL_GENERATION.load(SeqCst);
                let claim = future.facts.claim(&proof, cursor, trace.generation);
                trace.test(12, claim)
            };
        if accepted {
            IDENTITY.set(future.facts.cpu);
            RENDERED.set(0);
            FUTURE_CLAIMED.set(true);
            claimed = true;
        } else {
            refused = future.facts.refusal == 0;
            future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
        }
    }
    if refused {
        downstream_receipt("swap-claim", &f, &proof, &trace, cursor);
    }
    if !claimed {
        let _ = drain_future();
    }
}

#[derive(Clone, Copy)]
struct JobScope {
    parent: bool,
    parent_source: u64,
    // The entry snapshot, reread in place at completion as the native proof always was.
    native: InputState,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_job_end_enter(
    caller: usize,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let previous = JOBS.fetch_add(1, SeqCst);
    JOB_DEPTH.set(JOB_DEPTH.get() + 1);
    let parent = JOB_SELECTED.get();
    let parent_source = WORKER_SOURCE.get();
    let c = render_copy();
    let mut proof = c.key;
    let mut native = InputState::ZERO;
    JOB_SELECTED.set(
        enabled()
            && previous == 0
            && JOB_DEPTH.get() == 1
            && caller == image() + native::JOB_CALLER
            && native_cycle(&mut proof, &mut native, false, CyclePhase::Other)
            && proof == c.key,
    );
    WORKER_SOURCE.set(0);
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_JOB_END,
        JobScope {
            parent,
            parent_source,
            native,
        },
    );
}

/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_job_end_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(JobScope {
        parent,
        parent_source,
        mut native,
    }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_JOB_END, true)
    else {
        return;
    };
    let f = future_copy();
    let worker_source = WORKER_SOURCE.get();
    if worker_source != 0 && f.facts.cpu.source == worker_source {
        let current = render_copy();
        let mut proof = f.facts.predecessor;
        let mut cursor = 0u32;
        let mut trace = PhaseChecks::new();
        let valid = trace.test(0, enabled())
            && trace.test(
                1,
                native_cycle(
                    &mut proof,
                    &mut native,
                    current.joining,
                    CyclePhase::JobComplete,
                ),
            )
            && trace.test(2, proof == f.facts.predecessor)
            && trace.test(3, native == f.state)
            && {
                trace.generation = POLL_GENERATION.load(SeqCst);
                let unchanged = trace.generation == f.facts.generation;
                trace.test(4, unchanged)
            }
            && trace.test(
                5,
                load(&mut cursor, native.receiver + native::RECEIVER_CURSOR),
            )
            && trace.test(6, cursor == f.facts.cursor);
        let mut refused = false;
        {
            let mut w = worker();
            if w.future.facts.cpu.source == worker_source {
                let completed = valid && {
                    let complete = w.future.facts.complete();
                    trace.test(7, complete)
                };
                if !completed {
                    refused = w.future.facts.refusal == 0;
                    w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
                w.future.busy = false;
            }
        }
        if refused {
            downstream_receipt("job-return", &f, &proof, &trace, cursor);
        }
        let _ = drain_future();
    }
    WORKER_SOURCE.set(parent_source);
    JOB_SELECTED.set(parent);
    JOB_DEPTH.set(JOB_DEPTH.get() - 1);
    JOBS.fetch_sub(1, SeqCst);
    leave();
}

#[derive(Clone, Copy)]
struct WrapperScope {
    parent: bool,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_wrapper_enter(
    caller: usize,
    main: *mut c_void,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let parent = WRAPPER_SELECTED.get();
    WRAPPER_DEPTH.set(WRAPPER_DEPTH.get() + 1);
    let mut actual_main = 0usize;
    WRAPPER_SELECTED.set(
        JOB_SELECTED.get()
            && WRAPPER_DEPTH.get() == 1
            && caller == image() + native::WRAPPER_CALLER
            && load(&mut actual_main, image() + native::MAIN)
            && actual_main == main as usize,
    );
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_WRAPPER,
        WrapperScope { parent },
    );
}

/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_wrapper_exit(scope: *mut rsf_fallout4_cpu_scope) {
    let Some(WrapperScope { parent }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_WRAPPER, true)
    else {
        return;
    };
    WRAPPER_SELECTED.set(parent);
    WRAPPER_DEPTH.set(WRAPPER_DEPTH.get() - 1);
    leave();
}

#[derive(Clone, Copy)]
struct JoinScope {
    key: WorkerCycle,
    entry_proof: WorkerCycle,
    entry_trace: PhaseChecks,
    // The entry snapshot, reread in place after the wait as the native proof always was.
    native: InputState,
    selected: bool,
}

/// # Safety
/// `scope` is the shell's receipt for this call.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_worker_join_enter(
    caller: usize,
    scope: *mut rsf_fallout4_cpu_scope,
) {
    enter();
    let c = render_copy();
    let mut proof = c.key;
    let mut native = InputState::ZERO;
    let mut entry_trace = PhaseChecks::new();
    let selected = entry_trace.test(0, enabled())
        && entry_trace.test(1, caller == image() + native::JOIN_CALLER)
        && entry_trace.test(2, IDLE_DEPTH.get() == 1)
        && entry_trace.test(3, SWAP_DEPTH.get() == 0)
        && entry_trace.test(4, c.swapped)
        && entry_trace.test(5, c.closed)
        && entry_trace.test(6, c.tid == thread_id())
        && entry_trace.test(7, c.key.main == IDLE_MAIN.get())
        && entry_trace.test(8, c.key.idle == IDLE_SERIAL.get())
        && entry_trace.test(
            9,
            native_cycle(&mut proof, &mut native, false, CyclePhase::JoinEntry),
        )
        && entry_trace.test(10, proof == c.key);
    if selected {
        let mut w = worker();
        if w.current.key == c.key {
            w.current.joining = true;
        }
    }
    open_scope(
        scope,
        RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN,
        JoinScope {
            key: c.key,
            entry_proof: proof,
            entry_trace,
            native,
            selected,
        },
    );
}

/// `result` is the native join's EAX.
///
/// # Safety
/// `scope` is the receipt its entry wrote.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_worker_join_exit(
    scope: *mut rsf_fallout4_cpu_scope,
    result: u32,
) {
    let Some(JoinScope {
        key,
        entry_proof,
        entry_trace,
        mut native,
        selected,
    }) = scope_value(scope, RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN, true)
    else {
        return;
    };
    let f = future_copy();
    let mut proof = key;
    let (mut cursor, mut sequence, mut mode) = (0u32, 0u32, 0u32);
    // Early/no-op native RET leaves EAX undefined. Only selected proves the nonnull mode1 branch
    // that actually tail-calls WaitForSingleObjectEx.
    let mut trace = PhaseChecks::new();
    let valid = trace.test(0, selected)
        && trace.test(1, enabled())
        && trace.test(2, result == WAIT_OBJECT_0)
        && trace.test(
            3,
            native_cycle(&mut proof, &mut native, true, CyclePhase::JoinReturn),
        )
        && trace.test(4, proof == key)
        && trace.test(5, load(&mut mode, proof.worker + native::WORKER_MODE))
        && trace.test(6, mode == 0)
        && trace.test(7, native == f.state)
        && trace.test(
            8,
            load(&mut cursor, native.receiver + native::RECEIVER_CURSOR),
        )
        && trace.test(9, cursor == f.facts.cursor)
        && trace.test(
            10,
            load(&mut sequence, native.queue + native::QUEUE_SEQUENCE),
        )
        && {
            trace.generation = POLL_GENERATION.load(SeqCst);
            let unchanged = trace.generation == f.facts.generation;
            trace.test(12, unchanged)
        };
    trace.queue = sequence;
    trace.result_known = selected;
    if trace.result_known {
        trace.native_result = result;
    }
    trace.mode_known = trace.passed & (1 << 5) != 0;
    if trace.mode_known {
        trace.native_mode = mode;
    }
    let mut refused = false;
    {
        let mut w = worker();
        if selected && w.current.key == key {
            if w.future.facts.active() {
                let joined = valid && trace.test(13, !w.future.busy) && {
                    let join = w.future.facts.join(&proof);
                    trace.test(14, join)
                };
                if !joined {
                    refused = w.future.facts.refusal == 0;
                    w.future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
                }
            }
            w.current.joining = false;
        }
    }
    if !selected {
        downstream_receipt("join-unselected", &f, &entry_proof, &entry_trace, 0);
    } else if refused {
        downstream_receipt("join-return", &f, &proof, &trace, cursor);
    }
    let _ = drain_future();
    leave();
}

/// The PeekMessage continuation unwound before it returned a message.
#[unsafe(no_mangle)]
extern "C" fn rsf_fallout4_cpu_peek_cancel() {
    leave();
}

/// Retires an interrupted call without recording a normal return.
///
/// # Safety
/// The matching detour entry wrote the receipt.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_cancel(hook: u32, scope: *mut rsf_fallout4_cpu_scope) {
    const ABORT: u32 = RSF_GAME_CPU_ABORT_STATE_CHANGED;
    match hook {
        RSF_FALLOUT4_CPU_HOOK_IDLE => unsafe { rsf_fallout4_cpu_idle_exit(scope) },
        RSF_FALLOUT4_CPU_HOOK_CONSUME => unsafe { rsf_fallout4_cpu_consume_exit(scope) },
        RSF_FALLOUT4_CPU_HOOK_WRAPPER => unsafe { rsf_fallout4_cpu_wrapper_exit(scope) },
        RSF_FALLOUT4_CPU_HOOK_POLL => {
            let Some(PollScope { receipt, mapped }) = scope_value(scope, hook, true) else {
                return;
            };
            let p = pending();
            if receipt.candidate && p.source != 0 && p.generation == receipt.generation {
                abort_pending(ABORT);
            }
            if mapped {
                invalidate_worker(ABORT, false, None);
            }
            POLLS.fetch_sub(1, SeqCst);
            leave();
        }
        RSF_FALLOUT4_CPU_HOOK_SWAP => {
            if let Some(SwapScope { owns: true, .. }) = scope_value(scope, hook, false) {
                abort_pending(ABORT);
                invalidate_worker(ABORT, true, None);
                if FUTURE_CLAIMED.get() {
                    IDENTITY.set(CpuIdentity::ZERO);
                    RENDERED.set(0);
                    FUTURE_CLAIMED.set(false);
                } else {
                    close_frame();
                }
                PENDING.set(Pending::ZERO);
            }
            unsafe { rsf_fallout4_cpu_swap_leave(scope) };
        }
        RSF_FALLOUT4_CPU_HOOK_PLAYER_CONTROLS => {
            let Some(PlayerControlsScope {
                worker_consuming,
                main_source,
            }) = scope_value(scope, hook, true)
            else {
                return;
            };
            if worker_consuming {
                invalidate_worker(ABORT, false, None);
            } else if main_source != 0 && pending().source == main_source {
                abort_pending(ABORT);
            }
            leave();
        }
        RSF_FALLOUT4_CPU_HOOK_JOB_END => {
            let Some(JobScope {
                parent,
                parent_source,
                ..
            }) = scope_value(scope, hook, true)
            else {
                return;
            };
            {
                let mut w = worker();
                let source = WORKER_SOURCE.get();
                if source != 0 && w.future.facts.cpu.source == source {
                    w.future.facts.refuse(ABORT);
                    w.future.busy = false;
                }
            }
            let _ = drain_future();
            WORKER_SOURCE.set(parent_source);
            JOB_SELECTED.set(parent);
            JOB_DEPTH.set(JOB_DEPTH.get() - 1);
            JOBS.fetch_sub(1, SeqCst);
            leave();
        }
        RSF_FALLOUT4_CPU_HOOK_WORKER_JOIN => {
            let Some(JoinScope { key, selected, .. }) = scope_value(scope, hook, true) else {
                return;
            };
            if selected {
                let mut w = worker();
                if w.current.key == key {
                    w.future.facts.refuse(ABORT);
                    w.current.joining = false;
                }
            }
            let _ = drain_future();
            leave();
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Render producer queries, on the Swap thread.

fn current_identity() -> CpuIdentity {
    if SWAP_ADMITTED.get() && SWAP_DEPTH.get() == 1 && IDLE_DEPTH.get() <= 1 {
        IDENTITY.get()
    } else {
        CpuIdentity::ZERO
    }
}

/// The identity is this Swap's rendered source.
fn same_rendered_identity(expected: &CpuIdentity) -> bool {
    let identity = IDENTITY.get();
    RENDERED.get() == 1 && expected.source != 0 && expected.submission != 0 && *expected == identity
}

fn swap_scope() -> bool {
    enabled() && SWAP_ADMITTED.get() && SWAP_DEPTH.get() == 1 && IDLE_DEPTH.get() <= 1
}

/// Existing render association only: later input receipts do not erase rendered resources. This
/// does not certify sampled input, latency markers or FG eligibility.
fn render_source_is_current(expected: &CpuIdentity) -> bool {
    if !swap_scope() || FUTURE_CLAIMED.get() || !same_rendered_identity(expected) {
        return false;
    }
    let c = render_copy();
    let base = image();
    let (mut native_frame, mut main) = (0u32, 0usize);
    c.constructed
        && !c.closed
        && c.tid == thread_id()
        && c.cpu.session == expected.session
        && c.cpu.source == expected.source
        && c.cpu.submission == expected.submission
        && load(&mut native_frame, base + native::NATIVE_FRAME)
        && native_frame == c.key.frame
        && load(&mut main, base + native::MAIN)
        && main == c.key.main
}

/// The caller separately proves its WorldBegin parent. Reserved sources also require strict
/// pre-bind poll/raw ownership. A sealed rendered predecessor survives only its explicitly mapped
/// successor; unmatched changes refuse. A rendered fallback does not assert input ownership or
/// classify gameplay.
fn cpu_source_is_current(expected: &CpuIdentity) -> bool {
    if !swap_scope()
        || SOURCE_INVALID.get()
        || FUTURE_CLAIMED.get()
        || (pending().source != 0 && invalid_pending() != 0)
        || !same_rendered_identity(expected)
    {
        return false;
    }
    let c = render_copy();
    if c.cpu.source != IDENTITY.get().source || c.invalid {
        return false;
    }
    c.binding.source == 0 || bound_current_valid(&c, false)
}

/// Called once by the primary render producer inside the active Swap scope. Supplies a
/// render-pass association, never an ExecuteCommandLists fact.
fn begin_render() -> bool {
    if !swap_scope()
        || IDENTITY.get().source == 0
        || SOURCE_INVALID.get()
        || RENDERED.get() != 0
        || (pending().source != 0 && invalid_pending() != 0)
        || !drain_current_abort()
    {
        return false;
    }
    let f = future_copy();
    let identity = IDENTITY.get();
    let mut proof = WorkerCycle::ZERO;
    let mut native = InputState::ZERO;
    let (mut cursor, mut sequence) = (0u32, 0u32);
    proof.session = identity.session;
    proof.source = identity.source;
    proof.idle = IDLE_SERIAL.get();
    proof.main = IDLE_MAIN.get();
    let native_valid = native_cycle(&mut proof, &mut native, false, CyclePhase::WorldAdopt);
    let claimed = FUTURE_CLAIMED.get();
    let mut trace = PhaseChecks::new();
    // The queue tail may contain later input. Only a changed consumed cursor or poll generation
    // invalidates this completed sample's handoff.
    let adoption_valid = trace.test(0, claimed)
        && trace.test(1, native_valid)
        && trace.test(2, native == f.state)
        && trace.test(3, POLLS.load(SeqCst) == 0)
        && trace.test(
            4,
            load(&mut cursor, native.receiver + native::RECEIVER_CURSOR),
        )
        && trace.test(
            5,
            load(&mut sequence, native.queue + native::QUEUE_SEQUENCE),
        )
        && trace.test(6, cursor == f.facts.cursor);
    trace.queue = sequence;
    let tid = thread_id();
    let (mut failed, mut refused) = (false, false);
    {
        let mut w = worker();
        if w.current.aborting || w.current.abort_pending {
            return false;
        }
        let mut binding = Pending::ZERO;
        if claimed {
            let future = &mut w.future;
            let adopted = adoption_valid
                && trace.test(7, future.facts.cpu.source == identity.source)
                && trace.test(8, !future.busy)
                && trace.test(9, !future.retiring)
                && {
                    trace.generation = POLL_GENERATION.load(SeqCst);
                    let adopt = future.facts.adopt(&proof, cursor, trace.generation);
                    trace.test(10, adopt)
                };
            if adopted {
                binding = ticket_pending(future);
                *future = Future::ZERO;
                FUTURE_CLAIMED.set(false);
            } else {
                refused = future.facts.refusal == 0;
                future.facts.refuse(RSF_GAME_CPU_ABORT_STATE_CHANGED);
                failed = true;
            }
        } else {
            let p = pending();
            if p.bound && p.source == identity.source {
                binding = p;
            }
        }
        if !failed {
            let mut identity = IDENTITY.get();
            identity.submission = NEXT_SUBMISSION.fetch_add(1, SeqCst);
            IDENTITY.set(identity);
            RENDERED.set(1);
            w.current = RenderCycle {
                key: proof,
                cpu: identity,
                state: native,
                binding,
                tid,
                constructed: native_valid,
                ..RenderCycle::ZERO
            };
        }
    }
    if refused {
        downstream_receipt("world-adopt", &f, &proof, &trace, cursor);
    }
    if failed {
        let _ = drain_future();
        SOURCE_INVALID.set(true);
        return false;
    }
    true
}

/// Called only by the guarded native Present detour after verified owner/screen checks and
/// SUBMISSION completion. These are native phase facts.
fn begin_present_phase(source: &CpuIdentity, native_frame: u64, window: u64) -> bool {
    let source_current = cpu_source_is_current(source);
    let c = render_copy();
    let mut proof = c.key;
    let mut native = InputState::ZERO;
    let native_valid = native_cycle(&mut proof, &mut native, false, CyclePhase::Present);
    let bits = predicate_bits(&[
        enabled(),
        source_current,
        c.constructed,
        !c.closed,
        !c.present,
        native_frame == u64::from(c.key.frame),
        window == c.key.window as u64,
        c.tid == thread_id(),
        native_valid,
        proof == c.key,
        native == c.state,
    ]);
    if bits != 0x7ff {
        gate_receipt(2, bits, 0x7ff, &c, &proof, source);
        return false;
    }
    let mut w = worker();
    if w.current.key != c.key || w.current.invalid || w.current.closed {
        return false;
    }
    w.current.sealed = true;
    w.current.present = true;
    true
}

fn end_present_phase(source: &CpuIdentity) {
    let mut w = worker();
    if w.current.cpu.session == source.session && w.current.cpu.source == source.source {
        w.current.present = false;
    }
}

fn identity_from(value: *const rsf_fallout4_cpu_identity) -> Option<CpuIdentity> {
    unsafe { value.as_ref() }.copied()
}

/// # Safety
/// `out` is null or writable.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fallout4_cpu_current_identity(
    out: *mut rsf_fallout4_cpu_identity,
) {
    if let Some(out) = unsafe { out.as_mut() } {
        *out = current_identity();
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_fallout4_cpu_begin_render() -> bool {
    begin_render()
}

/// # Safety
/// `identity` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_source_is_current(
    identity: *const rsf_fallout4_cpu_identity,
) -> bool {
    identity_from(identity).is_some_and(|identity| cpu_source_is_current(&identity))
}

/// # Safety
/// `identity` is null or readable.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn rsf_fallout4_cpu_render_source_is_current(
    identity: *const rsf_fallout4_cpu_identity,
) -> bool {
    identity_from(identity).is_some_and(|identity| render_source_is_current(&identity))
}

/// # Safety
/// `identity` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_begin_present_phase(
    identity: *const rsf_fallout4_cpu_identity,
    native_frame: u64,
    window: u64,
) -> bool {
    identity_from(identity)
        .is_some_and(|identity| begin_present_phase(&identity, native_frame, window))
}

/// # Safety
/// `identity` is null or readable.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_fallout4_cpu_end_present_phase(
    identity: *const rsf_fallout4_cpu_identity,
) {
    if let Some(identity) = identity_from(identity) {
        end_present_phase(&identity);
    }
}

// ---------------------------------------------------------------------------------------------
// The PeekMessageA import cell.

// Never changed or cleared after publication: another IAT owner can retain our thunk.
static ORIGINAL_PEEK: AtomicUsize = AtomicUsize::new(0);

struct PeekCell {
    prepared: bool,
    owned: bool,
    protection_pending: bool,
    protection: u32,
}

static PEEK: Mutex<PeekCell> = Mutex::new(PeekCell {
    prepared: false,
    owned: false,
    protection_pending: false,
    protection: 0,
});

fn peek_cell() -> MutexGuard<'static, PeekCell> {
    PEEK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn peek_slot() -> *mut *mut c_void {
    (image() + native::PEEK_SLOT) as *mut *mut c_void
}

fn finish_peek_protection(cell: &mut PeekCell) -> bool {
    if !cell.protection_pending {
        return true;
    }
    let mut ignored = 0;
    if unsafe {
        VirtualProtect(
            peek_slot().cast_const().cast(),
            size_of::<usize>(),
            cell.protection,
            &mut ignored,
        )
    } == 0
    {
        return false;
    }
    cell.protection_pending = false;
    true
}

fn exchange_peek(
    cell: &mut PeekCell,
    expected: usize,
    desired: usize,
    exchanged: &mut bool,
) -> bool {
    *exchanged = false;
    if !finish_peek_protection(cell) {
        return false;
    }
    let mut protection = 0;
    if unsafe {
        VirtualProtect(
            peek_slot().cast_const().cast(),
            size_of::<usize>(),
            PAGE_READWRITE,
            &mut protection,
        )
    } == 0
    {
        return false;
    }
    cell.protection = protection;
    cell.protection_pending = true;
    // SAFETY: the import cell is an aligned, now writable pointer slot of the executable.
    let slot = unsafe { AtomicPtr::from_ptr(peek_slot()) };
    *exchanged = slot
        .compare_exchange(
            expected as *mut c_void,
            desired as *mut c_void,
            SeqCst,
            SeqCst,
        )
        .is_ok();
    finish_peek_protection(cell)
}

fn prepare_peek(host: &rsf_game_host_services) -> bool {
    let mut cell = peek_cell();
    let thunk = peek_thunk();
    let mut current = 0usize;
    if cell.owned
        || cell.protection_pending
        || thunk == 0
        || !native::notification_guards()
        || !load(&mut current, image() + native::PEEK_SLOT)
        || current == 0
        || current == thunk
    {
        return false;
    }
    let saved = ORIGINAL_PEEK.load(Acquire);
    // Refuse ambiguous restart when a later hook may already chain our resident thunk.
    if saved != 0 && saved != current {
        return false;
    }
    // SAFETY: the buffer is plain data; all-zero is its empty state.
    let mut region: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let executable =
        PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;
    if unsafe {
        VirtualQuery(
            current as *const c_void,
            &mut region,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } == 0
        || region.State != MEM_COMMIT
        || region.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
        || region.Protect & executable == 0
    {
        return false;
    }
    let callback = host
        .cpu_event
        .map_or(std::ptr::null(), |callback| callback as *const c_void);
    if !pin(current as *const c_void) || !pin(thunk as *const c_void) || !pin(callback) {
        return false;
    }
    ORIGINAL_PEEK.store(current, Release);
    cell.prepared = true;
    true
}

fn install_peek() -> bool {
    // Instruction guards were checked before enabling the receiver detour. The live import cell
    // is checked atomically at publication instead.
    let mut cell = peek_cell();
    if !cell.prepared {
        return false;
    }
    let mut exchanged = false;
    let protection_ok = exchange_peek(
        &mut cell,
        ORIGINAL_PEEK.load(Acquire),
        peek_thunk(),
        &mut exchanged,
    );
    cell.owned = exchanged;
    protection_ok && exchanged
}

fn retire_peek() -> bool {
    let mut cell = peek_cell();
    if !finish_peek_protection(&mut cell) {
        return false;
    }
    if cell.owned {
        let mut exchanged = false;
        let protection_ok = exchange_peek(
            &mut cell,
            peek_thunk(),
            ORIGINAL_PEEK.load(Acquire),
            &mut exchanged,
        );
        // A successful exchange restores the original; a failed one preserves the later writer.
        // In either case the immutable continuation and pinned thunk remain resident.
        if !protection_ok {
            return false;
        }
        cell.owned = false;
    }
    cell.prepared = false;
    true
}

// ---------------------------------------------------------------------------------------------
// Lifecycle, called under the session lock.

const HOOKS: std::ops::Range<u32> = 0..RSF_FALLOUT4_CPU_HOOK_COUNT;

pub(crate) fn busy() -> bool {
    ENTERED.load(Acquire) != 0
}

/// Checks the build, the guards and the services, then creates every detour disabled.
pub(crate) fn prepare(
    ops: &Ops,
    services: &rsf_game_host_services,
    _build: &creation::ExactBuild,
) -> bool {
    if platform().is_none()
        || services.struct_size < size_of::<rsf_game_host_services>() as u32
        || services.abi_version != RSF_GAME_ABI_VERSION
        || services.session_id == 0
        || services.cpu_event.is_none()
        || busy()
        || !native::guards()
    {
        return false;
    }
    if HOOKS.clone().any(hook_created) {
        return false;
    }
    HOST.lock().unwrap_or_else(PoisonError::into_inner).0 = *services;
    if !prepare_peek(services) {
        return false;
    }
    for hook in HOOKS {
        if create_hook(hook, hook_target(hook)) {
            continue;
        }
        let _ = stop(ops);
        return false;
    }
    true
}

pub(crate) fn start(ops: &Ops) -> bool {
    if !native::guards() || !native::notification_guards() {
        return false;
    }
    if !HOOKS.clone().all(hook_created) {
        return false;
    }
    ENABLED.store(true, Release);
    for hook in HOOKS {
        if ops.enable_hook(hook_target(hook) as usize) {
            continue;
        }
        quiesce();
        for installed in HOOKS {
            let _ = ops.disable_hook(hook_target(installed) as usize);
        }
        return false;
    }
    if install_peek() {
        return true;
    }
    quiesce();
    let _ = retire_peek();
    for hook in HOOKS {
        let _ = ops.disable_hook(hook_target(hook) as usize);
    }
    false
}

pub(crate) fn quiesce() {
    ENABLED.store(false, Release);
    invalidate_worker(RSF_GAME_CPU_ABORT_QUIESCE, true, None);
}

/// Restores the import cell, drains every obligation and removes every detour. False leaves the
/// remaining hooks for a later stop.
pub(crate) fn stop(ops: &Ops) -> bool {
    quiesce();
    if !retire_peek() || busy() {
        return false;
    }
    invalidate_worker(RSF_GAME_CPU_ABORT_QUIESCE, true, None);
    if !drain_future() || !drain_current_abort() {
        return false;
    }
    if platform().is_none() {
        return false;
    }
    for hook in HOOKS {
        if !hook_created(hook) {
            continue;
        }
        let target = hook_target(hook);
        if !ops.disable_hook(target as usize) || busy() || !remove_hook(hook, target) {
            return false;
        }
    }
    worker().current = RenderCycle::ZERO;
    true
}
