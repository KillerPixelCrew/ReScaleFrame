// SPDX-License-Identifier: MIT
// The shared D3D11 frame tap. This owns hook admission, refresh and retirement, the plan and
// callback state, gates and substitution, classification, capture routing, and every reference the
// tap holds. runtime/graphics/src/frame_tap.cpp supplies the remaining typed detours and D3D11
// leaves through `rsf_frame_tap_platform`.
mod detours;

use crate::com_ref::ComRef;
use crate::constant_buffer_read::{MAP_WRITE_DISCARD, Map, MappedResource, Unmap, slot};
use crate::frame_tap_abi::*;
use crate::resource_roles::matching_rule;
use crate::resource_roles_abi::{
    rsf_frame_shape, rsf_role_rule, rsf_role_table, rsf_texture_facts,
};
use std::cell::{Cell, UnsafeCell};
use std::collections::BTreeMap;
use std::ffi::{CString, c_void};
use std::mem::{MaybeUninit, transmute_copy, zeroed};
use std::ptr::{null, null_mut};
use std::slice;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

type Handle = *mut c_void;
type Platform = rsf_frame_tap_platform;

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const ALREADY_INSTALLED: i32 = -3;
const NOT_INSTALLED: i32 = -4;
const PATCH_FAILED: i32 = -5;
const ROLE_SCENE_COLOR: u32 = 1;
const ROLE_DEPTH: u32 = 2;
const ROLE_MOTION: u32 = 3;
const ROLE_EXPOSURE: u32 = 4;
const ROLE_HISTORY: u32 = 5;
const LEAVE: u32 = 0;
const DIVERT_PATCH_ALPHA: u32 = 2;
// D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL, D3D11_MAP_WRITE_DISCARD, D3D11_BIND_CONSTANT_BUFFER.
const KEEP_TARGETS: u32 = u32::MAX;
const BIND_CONSTANT_BUFFER: u32 = 4;
// Slot 13 is Draw. One comparison against its replacement says whether the runtime has rewritten
// the work-submission family since the last look.
const SLOT_DRAW: usize = 13;

const HOOKS: usize = RSF_FRAME_TAP_HOOK_COUNT as usize;
const HOOK_VERTEX_CONSTANTS: usize = RSF_FRAME_TAP_HOOK_VERTEX_CONSTANTS as usize;
const HOOK_VIEWS: usize = RSF_FRAME_TAP_HOOK_VIEWS as usize;
const HOOK_PIXEL_CONSTANTS: usize = RSF_FRAME_TAP_HOOK_PIXEL_CONSTANTS as usize;
const HOOK_SAMPLERS: usize = RSF_FRAME_TAP_HOOK_SAMPLERS as usize;
const HOOK_DRAW: usize = RSF_FRAME_TAP_HOOK_DRAW as usize;
const HOOK_TARGETS: usize = RSF_FRAME_TAP_HOOK_TARGETS as usize;
const HOOK_TARGETS_AND_UAVS: usize = RSF_FRAME_TAP_HOOK_TARGETS_AND_UAVS as usize;
const HOOK_VIEWPORTS: usize = RSF_FRAME_TAP_HOOK_VIEWPORTS as usize;
const HOOK_SCISSORS: usize = RSF_FRAME_TAP_HOOK_SCISSORS as usize;
const OBJECT_LAYOUT: u32 = RSF_FRAME_TAP_OBJECT_LAYOUT as u32;
const OBJECT_VERTEX_SHADER: u32 = RSF_FRAME_TAP_OBJECT_VERTEX_SHADER as u32;
const OBJECT_PIXEL_SHADER: u32 = RSF_FRAME_TAP_OBJECT_PIXEL_SHADER as u32;
const OBJECT_BLEND: u32 = RSF_FRAME_TAP_OBJECT_BLEND as u32;
const OBJECT_DEPTH_STENCIL: u32 = RSF_FRAME_TAP_OBJECT_DEPTH_STENCIL as u32;

// Every slot D3D11 allows a stage. Unreal's scene textures alone can reach past slot 16.
const VIEWS: usize = 128;
const SAMPLERS: usize = 16;
const VIEWPORTS: usize = 16;
const TARGETS: u32 = 8;
const STAGE_CONSTANTS: usize = 14;
const VERTEX_BUFFERS: usize = 32;
const CONSTANT_SLOTS: usize = RSF_FRAME_TAP_CONSTANT_SLOTS as usize;
const WATCH_SLOTS: usize = RSF_FRAME_TAP_WATCH_SLOTS as usize;
const MAX_INPUTS: usize = RSF_FRAME_TAP_MAX_INPUTS as usize;
const SUBSTITUTIONS: usize = RSF_FRAME_TAP_MAX_SUBSTITUTIONS as usize;
const ROLE_RULES: usize = RSF_FRAME_TAP_MAX_ROLE_RULES as usize;
const CANDIDATE_SLOTS: usize = RSF_FRAME_TAP_CANDIDATE_SLOTS as usize;
const CANDIDATES: usize = 64;
// What a watch budget holds when the caller asked for no limit.
const UNLIMITED: u32 = u32::MAX;
const VERDICT_SLOTS: usize = 64;
const VERDICT_LIFETIME: u32 = 16384;
const PATCHED_BLENDS: usize = 32;
const NEAR_MISSES: u32 = 12;
const PENDING_MAPS: usize = 16;

// Counters live outside the lock: a mutex on every binding would put the tap on the game's hottest
// path for numbers nobody reads per frame.
const CALLS_SEEN: usize = 0;
const CALLS_INSPECTED: usize = 1;
const MOTION_SEEN: usize = 2;
const DEPTH_SEEN: usize = 3;
const EXPOSURE_SEEN: usize = 4;
const PASSES: usize = 5;
const RENDER_WIDTH: usize = 6;
const RENDER_HEIGHT: usize = 7;
const TARGET_DRAWS: usize = 8;
const INPUTS_SUBSTITUTED: usize = 9;
const TARGETS_REDIRECTED: usize = 10;
const GATES_OPENED: usize = 11;
const DEPTH_MISMATCHES: usize = 12;
const MISMATCH_TARGET_WIDTH: usize = 13;
const MISMATCH_TARGET_HEIGHT: usize = 14;
const MISMATCH_DEPTH_WIDTH: usize = 15;
const MISMATCH_DEPTH_HEIGHT: usize = 16;
const MISMATCH_DEPTH_FORMAT: usize = 17;
const CANDIDATE_DRAWS: usize = 18;
const DRAWS_DIVERTED: usize = 19;
const BLENDS_PATCHED: usize = 20;
const DIVERT_REFUSED: usize = 21;
const LAST_REFUSAL: usize = 22;
const DRAWS_OVERRIDDEN: usize = 23;
const COPIES_REDIRECTED: usize = 24;
const COPIES_MISMATCHED: usize = 25;
const UPDATES_WATCHED: usize = 26;
const GATES_DECLINED: usize = 27;
const COUNTERS: usize = 28;

const ZERO_FACTS: rsf_texture_facts = unsafe { zeroed() };
const ZERO_GEOMETRY: rsf_frame_tap_geometry = unsafe { zeroed() };
const ZERO_INPUT: rsf_frame_tap_input = unsafe { zeroed() };
const ZERO_PLAN: rsf_frame_tap_plan = unsafe { zeroed() };
impl Default for rsf_frame_tap_plan {
    fn default() -> Self {
        ZERO_PLAN
    }
}

const ZERO_RULE: rsf_role_rule = unsafe { zeroed() };
const ZERO_OPTIONS: rsf_frame_tap_options = unsafe { zeroed() };
const ZERO_VIEWPORT: rsf_frame_tap_viewport = unsafe { zeroed() };
const ZERO_RECT: rsf_frame_tap_rect = unsafe { zeroed() };

thread_local! {
    // Callbacks bind resources of their own, which come straight back through these hooks. A depth
    // rather than a flag: the substitution guards are taken inside an outer hook's guard, and a
    // flag would be cleared by the inner one while the outer hook still runs.
    static INSIDE: Cell<u32> = const { Cell::new(0) };
}

fn inside() -> bool {
    INSIDE.with(|depth| depth.get() != 0)
}

struct Reentry;

impl Reentry {
    fn enter() -> Self {
        INSIDE.with(|depth| depth.set(depth.get() + 1));
        Self
    }
}

impl Drop for Reentry {
    fn drop(&mut self) {
        INSIDE.with(|depth| depth.set(depth.get() - 1));
    }
}

// Hooks on the observed context that may read the plan or the render-thread state hold an
// admission, counted as the overlay cursor and observer Present hooks count their calls. set_plan
// and uninstall close a gate, which keeps new hooks out, and drain the admitted ones before they
// change anything, so no hook on another thread reads a plan being overwritten, forwards a
// substitute the caller is about to release, or borrows a shadow being retired.
//
// A control call made from a tap callback already holds an admission further up its own stack.
// It never waits: it takes the control lock only if it is free and proceeds only if no other
// thread is admitted, and is otherwise refused before any change. Its own admissions stay valid:
// readers never hold the plan across a callback, and its retirement waits for the outermost
// admission to end.
static ADMITTED: AtomicU32 = AtomicU32::new(0);
// Closed while a control entry drains and changes state. Nested calls of an admitted hook still
// pass, since the entry may be waiting for that very hook.
static GATE_CLOSED: AtomicBool = AtomicBool::new(false);
// The render state is retired by whoever ends the last admission after an uninstall. The lock
// keeps an install from seeding the shadow while it is being retired.
static RETIRE_PENDING: AtomicBool = AtomicBool::new(false);
static RETIRING: Mutex<()> = Mutex::new(());
// Serialises install, uninstall and set_plan. Hooks never take it, and an admitted caller never
// waits for it.
static CONTROL: Mutex<()> = Mutex::new(());

thread_local! {
    static OWN: Cell<u32> = const { Cell::new(0) };
}

fn admitted_here() -> bool {
    OWN.with(Cell::get) != 0
}

// The control lock, or none for a caller inside a hook while another control call holds it: the
// holder may be draining that caller's own admission.
fn control() -> Option<MutexGuard<'static, ()>> {
    if !admitted_here() {
        return Some(CONTROL.lock().unwrap_or_else(PoisonError::into_inner));
    }
    match CONTROL.try_lock() {
        Ok(lock) => Some(lock),
        Err(TryLockError::Poisoned(error)) => Some(error.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

fn retiring() -> MutexGuard<'static, ()> {
    RETIRING.lock().unwrap_or_else(PoisonError::into_inner)
}

// A control entry's exclusive section, with the gate closed until it ends.
struct Writer {
    _lock: MutexGuard<'static, ()>,
}

impl Drop for Writer {
    fn drop(&mut self) {
        GATE_CLOSED.store(false, SeqCst);
    }
}

// Close the gate and drain the admitted hooks. An ordinary caller waits for them. A caller inside a
// hook is refused when another thread is admitted; the gate reopens and nothing has changed.
fn drain(lock: MutexGuard<'static, ()>) -> Option<Writer> {
    GATE_CLOSED.store(true, SeqCst);
    let writer = Writer { _lock: lock };
    let own = OWN.with(Cell::get);
    if own != 0 {
        return (ADMITTED.load(SeqCst) == own).then_some(writer);
    }
    while ADMITTED.load(SeqCst) != 0 {
        std::thread::yield_now();
    }
    Some(writer)
}

struct Admission;

impl Admission {
    // Admitted only for the observed context while installed. Counted before the gate is read, and
    // a writer closes the gate before reading the count, so one of the two always sees the other.
    fn enter(context: Handle) -> Option<Self> {
        if !observed(context) {
            return None;
        }
        if admitted_here() {
            ADMITTED.fetch_add(1, SeqCst);
        } else {
            loop {
                while GATE_CLOSED.load(SeqCst) {
                    std::thread::yield_now();
                }
                ADMITTED.fetch_add(1, SeqCst);
                if !GATE_CLOSED.load(SeqCst) {
                    break;
                }
                // A writer that drains to zero retires on its own, so this never has to.
                ADMITTED.fetch_sub(1, SeqCst);
            }
        }
        OWN.with(|own| own.set(own.get() + 1));
        let admission = Self;
        (TAP.installed.load(SeqCst) && observed(context)).then_some(admission)
    }

    // Hand the admission to the detour, which returns it through `leave` after forwarding.
    unsafe fn carry(self, admitted: *mut u32) {
        std::mem::forget(self);
        unsafe { *admitted = 1 };
    }
}

impl Drop for Admission {
    fn drop(&mut self) {
        OWN.with(|own| own.set(own.get() - 1));
        if ADMITTED.fetch_sub(1, SeqCst) == 1 && RETIRE_PENDING.load(SeqCst) {
            retire_render();
        }
    }
}

// Caller function pointers held in atomics, as `Option<unsafe extern "C" fn>`.
unsafe fn load_fn<F: Copy>(slot: &AtomicPtr<c_void>) -> F {
    const { assert!(size_of::<F>() == size_of::<Handle>()) };
    unsafe { transmute_copy::<Handle, F>(&slot.load(Acquire)) }
}

unsafe fn store_fn<F: Copy>(slot: &AtomicPtr<c_void>, value: F) {
    const { assert!(size_of::<F>() == size_of::<Handle>()) };
    slot.store(unsafe { transmute_copy::<F, Handle>(&value) }, Release);
}

// The patched vtable. Originals stay after uninstall: a call that entered a hook before the
// table was restored still forwards, and a later install reads them from the vtable again.
struct Hooks {
    vtable: AtomicPtr<Handle>,
    context: UnsafeCell<Option<ComRef>>,
    sentinel: AtomicPtr<c_void>,
    slots: [AtomicUsize; HOOKS],
    // Null where the hook is not patched.
    replacements: [AtomicPtr<c_void>; HOOKS],
    originals: [AtomicPtr<c_void>; HOOKS],
    refreshes: AtomicU32,
}

// The control lock serializes writes and drops of the context reference.
unsafe impl Sync for Hooks {}

static HOOK_TABLE: Hooks = Hooks {
    vtable: AtomicPtr::new(null_mut()),
    context: UnsafeCell::new(None),
    sentinel: AtomicPtr::new(null_mut()),
    slots: [const { AtomicUsize::new(0) }; HOOKS],
    replacements: [const { AtomicPtr::new(null_mut()) }; HOOKS],
    originals: [const { AtomicPtr::new(null_mut()) }; HOOKS],
    refreshes: AtomicU32::new(0),
};

fn original(hook: usize) -> Handle {
    HOOK_TABLE.originals[hook].load(Acquire)
}

// Record what the slot holds as the original before replacing it, so a call that reaches the
// replacement finds its original.
fn patch(platform: &Platform, vtable: *mut Handle, hook: usize, replacement: Handle) -> bool {
    let slot = HOOK_TABLE.slots[hook].load(Relaxed);
    let current = unsafe { vtable.add(slot).read_volatile() };
    HOOK_TABLE.originals[hook].store(current, Release);
    unsafe { (platform.patch.unwrap())(vtable, slot as u32, replacement) != 0 }
}

// Put back the originals of patched hooks, in the given order.
fn restore(platform: &Platform, vtable: *mut Handle, hooks: impl Iterator<Item = usize>) -> bool {
    let mut restored = true;
    for hook in hooks {
        if HOOK_TABLE.replacements[hook].load(Acquire).is_null() {
            continue;
        }
        let slot = HOOK_TABLE.slots[hook].load(Relaxed) as u32;
        if unsafe { (platform.patch.unwrap())(vtable, slot, original(hook)) } != 0 {
            HOOK_TABLE.replacements[hook].store(null_mut(), Release);
        } else {
            restored = false;
        }
    }
    if restored {
        HOOK_TABLE.vtable.store(null_mut(), Release);
        unsafe { *HOOK_TABLE.context.get() = None };
    }
    restored
}

// The Windows runtime rewrites the immediate context's work-submission family of slots after
// flush-class calls and again on the next draw, dispatch, copy or clear, discarding the hooks
// (docs/research/d3d11-runtime-vtable-rewrite.md). Every hook checks the sentinel on the way in
// and re-applies the whole table, recording what the runtime wrote as the new original.
fn refresh(platform: &Platform) {
    if !TAP.installed.load(Acquire) {
        return;
    }
    // Refresh must never wait for a writer, which may be draining this very hook.
    let _lock = match CONTROL.try_lock() {
        Ok(lock) => lock,
        Err(TryLockError::Poisoned(error)) => error.into_inner(),
        Err(TryLockError::WouldBlock) => return,
    };
    let vtable = HOOK_TABLE.vtable.load(Relaxed);
    if TAP.installed.load(Acquire)
        && !vtable.is_null()
        && unsafe { vtable.add(SLOT_DRAW).read_volatile() } != HOOK_TABLE.sentinel.load(Relaxed)
    {
        refresh_slow(platform, vtable);
    }
}

#[cold]
fn refresh_slow(platform: &Platform, vtable: *mut Handle) {
    let mut rewritten = 0;
    for (hook, replacement) in HOOK_TABLE.replacements.iter().enumerate() {
        let replacement = replacement.load(Relaxed);
        if replacement.is_null() {
            continue;
        }
        let slot = HOOK_TABLE.slots[hook].load(Relaxed);
        if unsafe { vtable.add(slot).read_volatile() } == replacement {
            continue;
        }
        rewritten += u32::from(patch(platform, vtable, hook, replacement));
    }
    if rewritten != 0 {
        HOOK_TABLE.refreshes.fetch_add(1, Relaxed);
    }
}

struct Setup {
    options: rsf_frame_tap_options,
    rules: [rsf_role_rule; ROLE_RULES],
    rule_count: u32,
}

struct CandidateSet {
    entries: [AtomicPtr<c_void>; CANDIDATES],
    count: AtomicU32,
}

impl CandidateSet {
    const fn new() -> Self {
        Self {
            entries: [const { AtomicPtr::new(null_mut()) }; CANDIDATES],
            count: AtomicU32::new(0),
        }
    }

    // Published counts make a reader see an old set or a complete new one.
    fn any(&self, matches: impl Fn(Handle) -> bool) -> bool {
        let count = (self.count.load(Acquire) as usize).min(CANDIDATES);
        self.entries[..count]
            .iter()
            .any(|entry| matches(entry.load(Relaxed)))
    }
}

struct Candidates {
    armed: AtomicBool,
    layouts: CandidateSet,
    widget_targets: CandidateSet,
    shaders: CandidateSet,
}

struct Divert {
    armed: AtomicBool,
    layer_target: AtomicPtr<c_void>,
    // Borrowed from the layer, resolved once so the "already the layer" refusal is a compare.
    layer_texture: AtomicPtr<c_void>,
    layer_width: AtomicU32,
    layer_height: AtomicU32,
    verdict: AtomicPtr<c_void>,
    verdict_user: AtomicPtr<c_void>,
}

struct Held(Option<ComRef>);

// The view constants reference is only touched under the lock.
unsafe impl Send for Held {}

struct Tap {
    // Guards installation and the view constants, which survive being unbound before a pass reads
    // them.
    held: Mutex<Held>,
    installed: AtomicBool,
    context: AtomicPtr<c_void>,
    // Written by install while not installed, read without the lock afterwards.
    setup: UnsafeCell<Setup>,
    // Written under the lock, read on the render thread only while `plan_active`, which is set
    // after the plan is written and cleared before it is touched again.
    plan: UnsafeCell<rsf_frame_tap_plan>,
    plan_views: UnsafeCell<Vec<ComRef>>,
    plan_active: AtomicBool,
    // Moved by every set_plan, so a reader can tell a callback replaced the plan under it.
    plan_generation: AtomicU32,
    // Which of the plan's gates opened in the current frame.
    gates: [AtomicBool; SUBSTITUTIONS],
    // Whether the bound render target is one the plan replaced, which decides viewport scaling.
    target_substituted: AtomicBool,
    shadow_needed: AtomicBool,
    bound_target: AtomicPtr<c_void>,
    view_constants_hint: AtomicPtr<c_void>,
    input_watch: AtomicPtr<c_void>,
    input_watch_dirty: AtomicBool,
    research_draw: AtomicPtr<c_void>,
    research_before: AtomicPtr<c_void>,
    research_compute: AtomicPtr<c_void>,
    research_user: AtomicPtr<c_void>,
    candidates: Candidates,
    divert: Divert,
    constant_override: AtomicPtr<c_void>,
    constant_override_user: AtomicPtr<c_void>,
    constant_override_format: AtomicU32,
    verdict_epoch: AtomicU32,
    // Watched render targets, compared by address and never dereferenced.
    watch: [AtomicPtr<c_void>; WATCH_SLOTS],
    watch_budget: [AtomicU32; WATCH_SLOTS],
    sampler_bias_active: AtomicBool,
    counters: [AtomicU32; COUNTERS],
}

// Cross-thread state is atomic or locked; the cells follow the publication rules above.
unsafe impl Sync for Tap {}

static TAP: Tap = Tap {
    held: Mutex::new(Held(None)),
    installed: AtomicBool::new(false),
    context: AtomicPtr::new(null_mut()),
    setup: UnsafeCell::new(Setup {
        options: ZERO_OPTIONS,
        rules: [ZERO_RULE; ROLE_RULES],
        rule_count: 0,
    }),
    plan: UnsafeCell::new(ZERO_PLAN),
    plan_views: UnsafeCell::new(Vec::new()),
    plan_active: AtomicBool::new(false),
    plan_generation: AtomicU32::new(0),
    gates: [const { AtomicBool::new(false) }; SUBSTITUTIONS],
    target_substituted: AtomicBool::new(false),
    shadow_needed: AtomicBool::new(false),
    bound_target: AtomicPtr::new(null_mut()),
    view_constants_hint: AtomicPtr::new(null_mut()),
    input_watch: AtomicPtr::new(null_mut()),
    input_watch_dirty: AtomicBool::new(true),
    research_draw: AtomicPtr::new(null_mut()),
    research_before: AtomicPtr::new(null_mut()),
    research_compute: AtomicPtr::new(null_mut()),
    research_user: AtomicPtr::new(null_mut()),
    candidates: Candidates {
        armed: AtomicBool::new(false),
        layouts: CandidateSet::new(),
        widget_targets: CandidateSet::new(),
        shaders: CandidateSet::new(),
    },
    divert: Divert {
        armed: AtomicBool::new(false),
        layer_target: AtomicPtr::new(null_mut()),
        layer_texture: AtomicPtr::new(null_mut()),
        layer_width: AtomicU32::new(0),
        layer_height: AtomicU32::new(0),
        verdict: AtomicPtr::new(null_mut()),
        verdict_user: AtomicPtr::new(null_mut()),
    },
    constant_override: AtomicPtr::new(null_mut()),
    constant_override_user: AtomicPtr::new(null_mut()),
    constant_override_format: AtomicU32::new(0),
    verdict_epoch: AtomicU32::new(0),
    watch: [const { AtomicPtr::new(null_mut()) }; WATCH_SLOTS],
    watch_budget: [const { AtomicU32::new(0) }; WATCH_SLOTS],
    sampler_bias_active: AtomicBool::new(false),
    counters: [const { AtomicU32::new(0) }; COUNTERS],
};

fn held() -> MutexGuard<'static, Held> {
    TAP.held.lock().unwrap_or_else(PoisonError::into_inner)
}

fn setup() -> &'static Setup {
    unsafe { &*TAP.setup.get() }
}

fn options() -> &'static rsf_frame_tap_options {
    &setup().options
}

fn plan() -> &'static rsf_frame_tap_plan {
    unsafe { &*TAP.plan.get() }
}

fn observed(context: Handle) -> bool {
    context == TAP.context.load(Relaxed)
}

fn bump(counter: usize) {
    TAP.counters[counter].fetch_add(1, Relaxed);
}

fn record(counter: usize, value: u32) {
    TAP.counters[counter].store(value, Relaxed);
}

fn counter(counter: usize) -> u32 {
    TAP.counters[counter].load(Relaxed)
}

fn say(platform: &Platform, message: impl FnOnce() -> String) {
    let options = options();
    if options.log.is_none() {
        return;
    }
    if let Ok(message) = CString::new(message()) {
        unsafe { (platform.log.unwrap())(options.log, options.log_user, message.as_ptr()) };
    }
}

// What the pixel stage has bound, as far as the tap has seen it. Unreal binds a slot per call, so
// the set is shadowed across calls and judged at the draw. The texture behind each view is held.
struct Slot {
    view: Handle,
    texture: Option<ComRef>,
    facts: rsf_texture_facts,
    role: u32,
}

const EMPTY_SLOT: Slot = Slot {
    view: null_mut(),
    texture: None,
    facts: ZERO_FACTS,
    role: 0,
};

struct Override {
    active: bool,
    count: usize,
    slots: [u32; CONSTANT_SLOTS],
    originals: [Option<ComRef>; CONSTANT_SLOTS],
}

// Render-thread state of the observed context. Reached once the reentry depth says a call is the
// game's, so a callback's nested hooks never alias it. Uninstall reaches it from its own thread,
// as the native owner did.
struct Render {
    slots: [Slot; VIEWS],
    occupied: [u64; VIEWS / 64],
    // What the render thread last acted on of `shadow_needed`.
    shadow_live: bool,
    // Set when a binding changed, cleared when the set has been looked at at a draw.
    shadow_dirty: bool,
    // Edge trigger on the set becoming complete, once per frame because the set goes incomplete
    // between frames.
    signature_complete: bool,
    described: u32,
    input_watch_bound: bool,
    // Render target slot 0, shadowed so a draw knows its target without asking the context.
    target_view: Handle,
    target_texture: Option<ComRef>,
    target_facts: rsf_texture_facts,
    target_view_format: u32,
    // Draws into the current target since it was bound: a pass, not a frame.
    draws_into_target: u32,
    target_count: u32,
    depth_bound: bool,
    // Borrowed shadow of live IA/VS bindings, never retained as a deferred draw record.
    geometry_depth: Handle,
    geometry: rsf_frame_tap_geometry,
    geometry_valid: bool,
    pixel_constants: [Handle; STAGE_CONSTANTS],
    // Compared, never dereferenced; meaningful only while bound.
    pixel_shader: Handle,
    blend_state: Handle,
    depth_stencil_state: Handle,
    constant_override: Override,
    divert: Option<rsf_frame_tap_output>,
    // Created once per game blend state; never in a draw beyond the first.
    patched_blends: Vec<(Option<ComRef>, ComRef)>,
    // The game's own viewports and scissors, unscaled: the engine sets them per pass, not per
    // target, so a pass into an unreplaced target must get the game's numbers back.
    game_viewports: [rsf_frame_tap_viewport; VIEWPORTS],
    game_viewport_count: usize,
    game_scissors: [rsf_frame_tap_rect; VIEWPORTS],
    game_scissor_count: usize,
    // Per address: zero bytes when not a constant buffer the watch wants. Emptied when the epoch
    // moves and after a fixed number of lookups, since addresses are reused.
    verdicts: [(Handle, u32); VERDICT_SLOTS],
    verdict_lookups: u32,
    verdict_epoch_seen: u32,
}

struct RenderCell(UnsafeCell<Render>);

unsafe impl Sync for RenderCell {}

static RENDER: RenderCell = RenderCell(UnsafeCell::new(Render {
    slots: [EMPTY_SLOT; VIEWS],
    occupied: [0; VIEWS / 64],
    shadow_live: false,
    shadow_dirty: false,
    signature_complete: false,
    described: 0,
    input_watch_bound: false,
    target_view: null_mut(),
    target_texture: None,
    target_facts: ZERO_FACTS,
    target_view_format: 0,
    draws_into_target: 0,
    target_count: 0,
    depth_bound: false,
    geometry_depth: null_mut(),
    geometry: ZERO_GEOMETRY,
    geometry_valid: false,
    pixel_constants: [null_mut(); STAGE_CONSTANTS],
    pixel_shader: null_mut(),
    blend_state: null_mut(),
    depth_stencil_state: null_mut(),
    constant_override: Override {
        active: false,
        count: 0,
        slots: [0; CONSTANT_SLOTS],
        originals: [const { None }; CONSTANT_SLOTS],
    },
    divert: None,
    patched_blends: Vec::new(),
    game_viewports: [ZERO_VIEWPORT; VIEWPORTS],
    game_viewport_count: 0,
    game_scissors: [ZERO_RECT; VIEWPORTS],
    game_scissor_count: 0,
    verdicts: [(null_mut(), 0); VERDICT_SLOTS],
    verdict_lookups: 0,
    verdict_epoch_seen: 0,
}));

unsafe fn render() -> &'static mut Render {
    unsafe { &mut *RENDER.0.get() }
}

// A Map and its Unmap belong to a single observed context and installation. Both
// forwards are admitted, and retirement clears this table before admitting a new install.
#[derive(Clone, Copy)]
struct Pending {
    context: Handle,
    resource: Handle,
    data: Handle,
    bytes: u32,
}

// These addresses are only passed to the watch before the matched native Unmap.
unsafe impl Send for Pending {}

static PENDING: Mutex<[Option<Pending>; PENDING_MAPS]> = Mutex::new([None; PENDING_MAPS]);

fn pending() -> MutexGuard<'static, [Option<Pending>; PENDING_MAPS]> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner)
}

// Biased clones of the game's samplers, keyed by the game's sampler, which is pinned so its
// address cannot be reused while it keys an entry.
struct Biased {
    _pinned: ComRef,
    clone: Option<ComRef>,
    bias: f32,
}

struct Samplers {
    bias: f32,
    biased: BTreeMap<usize, Biased>,
    clones: BTreeMap<usize, Handle>,
}

// Only touched under the sampler lock.
unsafe impl Send for Samplers {}

static SAMPLER_STATE: Mutex<Samplers> = Mutex::new(Samplers {
    bias: 0.0,
    biased: BTreeMap::new(),
    clones: BTreeMap::new(),
});

fn sampler_state() -> MutexGuard<'static, Samplers> {
    SAMPLER_STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Samplers {
    // The game's object behind a binding, which is the clone's original when the bias put it there.
    fn original_of(&self, sampler: Handle) -> Handle {
        self.clones
            .get(&(sampler as usize))
            .copied()
            .unwrap_or(sampler)
    }

    // What to bind for one of the game's samplers at the current bias. The leaf decides which
    // filters are biased; a sampler it declines keeps passing through until the bias changes.
    fn biased(&mut self, platform: &Platform, context: Handle, original: Handle) -> Handle {
        if original.is_null() || self.bias == 0.0 {
            return original;
        }
        let bias = self.bias;
        let entry = self
            .biased
            .entry(original as usize)
            .or_insert_with(|| Biased {
                _pinned: unsafe { ComRef::retain(original) },
                clone: None,
                bias: 0.0,
            });
        if entry.bias == bias {
            return entry.clone.as_ref().map_or(original, ComRef::as_ptr);
        }
        if let Some(previous) = entry.clone.take() {
            self.clones.remove(&(previous.as_ptr() as usize));
        }
        entry.bias = bias;
        let clone = unsafe {
            ComRef::from_owned((platform.clone_sampler.unwrap())(context, original, bias))
        };
        let Some(clone) = clone else {
            return original;
        };
        let raw = clone.as_ptr();
        self.clones.insert(raw as usize, original);
        entry.clone = Some(clone);
        raw
    }
}

fn texture_behind(platform: &Platform, view: Handle) -> Option<ComRef> {
    unsafe { ComRef::from_owned((platform.texture_behind.unwrap())(view)) }
}

fn facts_of(platform: &Platform, texture: Handle) -> rsf_texture_facts {
    let mut facts = ZERO_FACTS;
    unsafe { (platform.texture_facts.unwrap())(texture, &mut facts) };
    facts
}

// The extent and format of what a view looks at, or none when it is not a 2D texture.
fn view_extent(platform: &Platform, view: Handle) -> Option<rsf_texture_facts> {
    let texture = texture_behind(platform, view)?;
    Some(facts_of(platform, texture.as_ptr()))
}

#[derive(Clone, Copy)]
enum View {
    Shader,
    Render,
}

// The plan entry's view that substitutes this texture right now: named by the plan, carrying that
// kind of view, and past its gate. Null otherwise.
fn applicable(texture: Handle, view: View) -> Handle {
    if texture.is_null() {
        return null_mut();
    }
    let plan = plan();
    let count = (plan.count as usize).min(SUBSTITUTIONS);
    let Some(index) = plan.items[..count]
        .iter()
        .position(|item| item.texture == texture)
    else {
        return null_mut();
    };
    let entry = &plan.items[index];
    let substitute = match view {
        View::Shader => entry.shader_view,
        View::Render => entry.render_view,
    };
    if substitute.is_null() || (!entry.after_target.is_null() && !TAP.gates[index].load(Relaxed)) {
        return null_mut();
    }
    substitute
}

fn close_gates() {
    for gate in &TAP.gates {
        gate.store(false, Relaxed);
    }
}

// Open any gate this render target opens, before the binding is forwarded: the gate is the moment
// the scene is finished and nothing downstream has read it. False when the gate callback replaced
// or cleared the plan, whose entries then no longer describe this binding. The plan is copied out
// rather than borrowed across the callback, which may set a plan on this thread.
fn open_gates_for(platform: &Platform, context: Handle, texture: Handle) -> bool {
    if texture.is_null() {
        return true;
    }
    let generation = TAP.plan_generation.load(SeqCst);
    let (count, after_targets, on_gate, on_gate_user) = {
        let plan = plan();
        let count = (plan.count as usize).min(SUBSTITUTIONS);
        (
            count,
            plan.items.map(|item| item.after_target),
            plan.on_gate,
            plan.on_gate_user,
        )
    };
    let (mut asked, mut accepted) = (false, true);
    for (index, &after_target) in after_targets[..count].iter().enumerate() {
        if after_target != texture || TAP.gates[index].load(Relaxed) {
            continue;
        }
        if on_gate.is_some() && !asked {
            // Under the caller's reentry guard. A decline leaves the gate shut for the next binding.
            accepted =
                unsafe { (platform.gate.unwrap())(on_gate, on_gate_user, context, texture) != 0 };
            asked = true;
            if TAP.plan_generation.load(SeqCst) != generation {
                return false;
            }
            if !accepted {
                bump(GATES_DECLINED);
            }
        }
        if !accepted {
            continue;
        }
        TAP.gates[index].store(true, Relaxed);
        bump(GATES_OPENED);
    }
    true
}

fn scale_viewports(
    source: &[rsf_frame_tap_viewport],
    scale_x: f32,
    scale_y: f32,
) -> [rsf_frame_tap_viewport; VIEWPORTS] {
    let mut scaled = [ZERO_VIEWPORT; VIEWPORTS];
    for (scaled, source) in scaled.iter_mut().zip(source) {
        *scaled = rsf_frame_tap_viewport {
            x: source.x * scale_x,
            y: source.y * scale_y,
            width: source.width * scale_x,
            height: source.height * scale_y,
            ..*source
        };
    }
    scaled
}

fn scale_scissors(
    source: &[rsf_frame_tap_rect],
    scale_x: f32,
    scale_y: f32,
) -> [rsf_frame_tap_rect; VIEWPORTS] {
    let mut scaled = [ZERO_RECT; VIEWPORTS];
    for (scaled, source) in scaled.iter_mut().zip(source) {
        *scaled = rsf_frame_tap_rect {
            left: (source.left as f32 * scale_x) as i32,
            top: (source.top as f32 * scale_y) as i32,
            right: (source.right as f32 * scale_x) as i32,
            bottom: (source.bottom as f32 * scale_y) as i32,
        };
    }
    scaled
}

// Put the viewports and scissors where the bound target needs them: the game's request scaled
// onto a replaced output resolution target, and the game's own numbers otherwise. The originals
// are called so this does not come back through the viewport hooks.
fn apply_viewport_policy(platform: &Platform, render: &Render, context: Handle) {
    let (scale_x, scale_y) = if TAP.target_substituted.load(Relaxed) {
        (plan().viewport_scale_x, plan().viewport_scale_y)
    } else {
        (1.0, 1.0)
    };
    let set_viewports = original(HOOK_VIEWPORTS);
    if render.game_viewport_count > 0 && !set_viewports.is_null() {
        let count = render.game_viewport_count;
        let scaled = scale_viewports(&render.game_viewports[..count], scale_x, scale_y);
        unsafe {
            (platform.set_viewports.unwrap())(set_viewports, context, count as u32, scaled.as_ptr())
        };
    }
    let set_scissors = original(HOOK_SCISSORS);
    if render.game_scissor_count > 0 && !set_scissors.is_null() {
        let count = render.game_scissor_count;
        let scaled = scale_scissors(&render.game_scissors[..count], scale_x, scale_y);
        unsafe {
            (platform.set_scissors.unwrap())(set_scissors, context, count as u32, scaled.as_ptr())
        };
    }
}

// The occupied slots of a snapshot, lowest first, so a visit may drop the slot it visits.
#[derive(Clone, Copy)]
struct Occupied([u64; VIEWS / 64]);

impl Iterator for Occupied {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        for (word, bits) in self.0.iter_mut().enumerate() {
            if *bits != 0 {
                let index = word * 64 + bits.trailing_zeros() as usize;
                *bits &= *bits - 1;
                return Some(index);
            }
        }
        None
    }
}

fn occupied(render: &Render) -> Occupied {
    Occupied(render.occupied)
}

fn set_occupied(render: &mut Render, index: usize, occupied: bool) {
    let bit = 1u64 << (index % 64);
    if occupied {
        render.occupied[index / 64] |= bit;
    } else {
        render.occupied[index / 64] &= !bit;
    }
}

fn drop_slot(render: &mut Render, index: usize) {
    render.slots[index] = EMPTY_SLOT;
    set_occupied(render, index, false);
}

fn slot_texture(render: &Render, index: usize) -> Handle {
    render.slots[index]
        .texture
        .as_ref()
        .map_or(null_mut(), ComRef::as_ptr)
}

fn target_texture(render: &Render) -> Handle {
    render
        .target_texture
        .as_ref()
        .map_or(null_mut(), ComRef::as_ptr)
}

fn forget_target(render: &mut Render) {
    render.target_texture = None;
    TAP.bound_target.store(null_mut(), Relaxed);
    render.target_facts = ZERO_FACTS;
    render.target_view = null_mut();
    render.draws_into_target = 0;
    render.target_view_format = 0;
}

// Record what render target slot 0 now holds. `resolved` is the texture behind `view` when the
// caller already took it.
fn shadow_target(platform: &Platform, render: &mut Render, view: Handle, resolved: Option<ComRef>) {
    if render.target_view == view {
        // The same view rebound is not a new pass, so the draw ordinal continues.
        return;
    }
    forget_target(render);
    render.target_view = view;
    if view.is_null() {
        return;
    }
    let texture = resolved.or_else(|| texture_behind(platform, view));
    if let Some(texture) = &texture {
        render.target_facts = facts_of(platform, texture.as_ptr());
        TAP.bound_target.store(texture.as_ptr(), Relaxed);
    }
    render.target_texture = texture;
    // For a typeless target the view, not the texture, says whether output is encoded.
    render.target_view_format = unsafe { (platform.target_view_format.unwrap())(view) };
}

// A pass that writes output slot zero cannot read it; clear that hazard from the shadow.
fn unbind_target_reads(render: &mut Render) {
    let target = target_texture(render);
    if target.is_null() {
        return;
    }
    for index in occupied(render) {
        if slot_texture(render, index) == target {
            drop_slot(render, index);
            render.shadow_dirty = true;
            TAP.input_watch_dirty.store(true, Relaxed);
        }
    }
}

// Is this draw one of the handful in the frame worth a second look? Pointer comparisons over the
// shadow, behind one load that rejects everything until something is named.
fn candidate_passes(render: &Render) -> bool {
    let candidates = &TAP.candidates;
    if !candidates.armed.load(Relaxed) {
        return false;
    }
    let layout = render.geometry.input_layout;
    if candidates.layouts.any(|entry| entry == layout) {
        return true;
    }
    let (pixel, vertex) = (render.pixel_shader, render.geometry.vertex_shader);
    if candidates
        .shaders
        .any(|entry| entry == pixel || entry == vertex)
    {
        return true;
    }
    (0..CANDIDATE_SLOTS).any(|slot| {
        let texture = slot_texture(render, slot);
        !texture.is_null() && candidates.widget_targets.any(|entry| entry == texture)
    })
}

// What the shadow knows of a draw. Inputs and constant buffers are the caller's to add.
fn common_facts(render: &Render, indexed: bool, elements: u32) -> rsf_frame_tap_target_draw {
    let mut facts: rsf_frame_tap_target_draw = unsafe { zeroed() };
    facts.struct_size = size_of::<rsf_frame_tap_target_draw>() as u32;
    facts.watch_index = WATCH_SLOTS as u32;
    facts.render_target = target_texture(render);
    facts.target_width = render.target_facts.width;
    facts.target_height = render.target_facts.height;
    facts.target_format = render.target_facts.format;
    facts.target_view_format = render.target_view_format;
    facts.target_count = render.target_count;
    facts.target_samples = render.target_facts.sample_count;
    facts.depth_bound = u32::from(render.depth_bound);
    facts.indexed = u32::from(indexed);
    facts.element_count = elements;
    facts.pixel_shader = render.pixel_shader;
    facts.vertex_shader = render.geometry.vertex_shader;
    facts.input_layout = render.geometry.input_layout;
    facts.blend_state = render.blend_state;
    facts.depth_stencil_state = render.depth_stencil_state;
    facts.vertex_stride = render.geometry.strides[0];
    facts.topology = render.geometry.topology;
    facts
}

// The facts a verdict or override is decided from, filled from the shadow before the draw. No
// viewport query: that call cannot be afforded before every candidate draw.
fn divert_facts(
    render: &Render,
    indexed: bool,
    elements: u32,
    inputs: &mut [rsf_frame_tap_input; MAX_INPUTS],
) -> rsf_frame_tap_target_draw {
    let mut facts = common_facts(render, indexed, elements);
    facts.vertex_constants = render.geometry.vertex_constants;
    facts.pixel_constants = render.pixel_constants;
    collect_inputs(render, &mut facts, inputs);
    facts
}

fn collect_inputs(
    render: &Render,
    draw: &mut rsf_frame_tap_target_draw,
    inputs: &mut [rsf_frame_tap_input; MAX_INPUTS],
) {
    let mut used = 0;
    for slot in occupied(render) {
        if used == MAX_INPUTS {
            draw.inputs_truncated = 1;
            break;
        }
        let facts = render.slots[slot].facts;
        inputs[used] = rsf_frame_tap_input {
            slot: slot as u32,
            texture: slot_texture(render, slot),
            width: facts.width,
            height: facts.height,
            format: facts.format,
        };
        used += 1;
    }
    draw.input_count = used as u32;
    draw.inputs = inputs.as_ptr();
}

fn refuse_divert(reason: u32) {
    bump(DIVERT_REFUSED);
    record(LAST_REFUSAL, reason);
}

// The game's blend with its alpha operations patched, created once and cached. Null when it
// cannot be built, and the caller then refuses the divert rather than lose the coverage.
fn patched_blend_for(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    original: Handle,
) -> Handle {
    if let Some((_, patched)) = render
        .patched_blends
        .iter()
        .find(|(game, _)| game.as_ref().map_or(null_mut(), ComRef::as_ptr) == original)
    {
        return patched.as_ptr();
    }
    if render.patched_blends.len() >= PATCHED_BLENDS {
        return null_mut();
    }
    let mut log: rsf_frame_tap_blend_log = unsafe { zeroed() };
    let Some(patched) = (unsafe {
        ComRef::from_owned((platform.patch_blend.unwrap())(context, original, &mut log))
    }) else {
        return null_mut();
    };
    let raw = patched.as_ptr();
    let original = (!original.is_null()).then(|| unsafe { ComRef::retain(original) });
    render.patched_blends.push((original, patched));
    bump(BLENDS_PATCHED);
    // Once per distinct state: a write mask without its alpha bit makes every alpha operation
    // decorative, and this line says whether a layer without coverage is the blend's fault.
    say(platform, || {
        format!(
            "divert: patched a blend. Before: enable {}, colour {}/{} op {}, alpha {}/{} op {}, \
             write mask 0x{:x}. After: alpha 2/6 op 1, write mask 0x{:x}",
            log.enable,
            log.source,
            log.destination,
            log.operation,
            log.alpha_source,
            log.alpha_destination,
            log.alpha_operation,
            log.write_mask,
            log.patched_write_mask
        )
    });
    raw
}

fn release_saved(saved: &rsf_frame_tap_output) {
    for object in saved
        .targets
        .into_iter()
        .chain([saved.depth_view, saved.blend])
    {
        drop(unsafe { ComRef::from_owned(object) });
    }
}

// Move this draw to the layer, if the verdict says it is the interface and nothing makes that
// unsafe. True when the saved state has to be put back after the draw.
fn begin_divert(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    facts: &rsf_frame_tap_target_draw,
) -> bool {
    let divert = &TAP.divert;
    let verdict_fn: rsf_frame_tap_verdict_fn = unsafe { load_fn(&divert.verdict) };
    if !divert.armed.load(Relaxed) || verdict_fn.is_none() {
        return false;
    }
    let verdict = unsafe {
        (platform.verdict.unwrap())(verdict_fn, divert.verdict_user.load(Relaxed), facts)
    };
    if verdict == LEAVE {
        return false;
    }
    let layer = divert.layer_target.load(Relaxed);
    if layer.is_null() {
        refuse_divert(RSF_FRAME_TAP_REFUSED_NO_LAYER);
        return false;
    }
    if render.target_count != 1 {
        refuse_divert(RSF_FRAME_TAP_REFUSED_MULTIPLE_TARGETS);
        return false;
    }
    let target = target_texture(render);
    if !target.is_null() && target == divert.layer_texture.load(Relaxed) {
        refuse_divert(RSF_FRAME_TAP_REFUSED_ALREADY_LAYER);
        return false;
    }

    // Saved from the context: what goes back is exactly what was bound, all eight targets.
    let mut saved: rsf_frame_tap_output = unsafe { zeroed() };
    unsafe { (platform.save_output.unwrap())(context, &mut saved) };
    let mut blend = saved.blend;
    if verdict == DIVERT_PATCH_ALPHA {
        blend = patched_blend_for(platform, render, context, saved.blend);
        if blend.is_null() {
            // A layer with colour and no coverage composites to nothing.
            release_saved(&saved);
            refuse_divert(RSF_FRAME_TAP_REFUSED_BLEND);
            return false;
        }
    }

    // No depth: the interface is an overlay on the layer, and the scene's depth has another extent.
    unsafe {
        (platform.set_targets.unwrap())(
            original(HOOK_TARGETS),
            context,
            1,
            &layer,
            null_mut(),
            null(),
        );
        (platform.set_blend.unwrap())(
            context,
            blend,
            saved.blend_factor.as_ptr(),
            saved.blend_mask,
        );
    }
    // Scaled by what the draw covered of its own target, so the fraction of the frame is kept.
    let (target_width, target_height) = (render.target_facts.width, render.target_facts.height);
    let layer_width = divert.layer_width.load(Relaxed);
    let layer_height = divert.layer_height.load(Relaxed);
    if saved.viewport_count >= 1
        && target_width != 0
        && target_height != 0
        && layer_width != 0
        && layer_height != 0
    {
        let scale_x = layer_width as f32 / target_width as f32;
        let scale_y = layer_height as f32 / target_height as f32;
        let count = (saved.viewport_count as usize).min(VIEWPORTS);
        let viewports = scale_viewports(&saved.viewports[..count], scale_x, scale_y);
        unsafe {
            (platform.set_viewports.unwrap())(null_mut(), context, count as u32, viewports.as_ptr())
        };
        if saved.scissor_count >= 1 {
            let count = (saved.scissor_count as usize).min(VIEWPORTS);
            let scissors = scale_scissors(&saved.scissors[..count], scale_x, scale_y);
            unsafe {
                (platform.set_scissors.unwrap())(
                    null_mut(),
                    context,
                    count as u32,
                    scissors.as_ptr(),
                )
            };
        }
    }
    render.divert = Some(saved);
    bump(DRAWS_DIVERTED);
    true
}

fn end_divert(platform: &Platform, render: &mut Render, context: Handle) {
    let Some(saved) = render.divert.take() else {
        return;
    };
    unsafe {
        (platform.set_targets.unwrap())(
            original(HOOK_TARGETS),
            context,
            TARGETS,
            saved.targets.as_ptr(),
            saved.depth_view,
            null(),
        );
        (platform.set_blend.unwrap())(
            context,
            saved.blend,
            saved.blend_factor.as_ptr(),
            saved.blend_mask,
        );
        if saved.viewport_count >= 1 {
            (platform.set_viewports.unwrap())(
                null_mut(),
                context,
                saved.viewport_count,
                saved.viewports.as_ptr(),
            );
        }
        if saved.scissor_count >= 1 {
            (platform.set_scissors.unwrap())(
                null_mut(),
                context,
                saved.scissor_count,
                saved.scissors.as_ptr(),
            );
        }
    }
    release_saved(&saved);
}

fn try_divert(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    indexed: bool,
    elements: u32,
) -> bool {
    if !TAP.divert.armed.load(Relaxed) || !candidate_passes(render) {
        return false;
    }
    let mut inputs = [ZERO_INPUT; MAX_INPUTS];
    let facts = divert_facts(render, indexed, elements, &mut inputs);
    begin_divert(platform, render, context, &facts)
}

// Geometry, hull and domain constants, asked only on a draw that is offered.
fn stage_constants(platform: &Platform, context: Handle, facts: &mut rsf_frame_tap_target_draw) {
    unsafe {
        (platform.stage_constants.unwrap())(
            context,
            facts.geometry_constants.as_mut_ptr(),
            facts.hull_constants.as_mut_ptr(),
            facts.domain_constants.as_mut_ptr(),
        )
    };
}

// Encoded slots are stage * 14 + slot: vertex, pixel, geometry, hull, domain.
fn bind_override(platform: &Platform, context: Handle, encoded: u32, buffer: Handle) {
    let stage = encoded / STAGE_CONSTANTS as u32;
    let setter = match stage {
        0 => original(HOOK_VERTEX_CONSTANTS),
        1 => original(HOOK_PIXEL_CONSTANTS),
        _ => null_mut(),
    };
    unsafe {
        (platform.bind_constants.unwrap())(
            setter,
            context,
            stage,
            encoded % STAGE_CONSTANTS as u32,
            buffer,
        )
    };
}

// Bind stand-in constant buffers for this draw if the override asks. After the divert declined.
fn bound_constant(context: Handle, index: usize) -> Option<ComRef> {
    type GetBuffers = unsafe extern "system" fn(Handle, u32, u32, *mut Handle);
    // Windows SDK ID3D11DeviceContext VS, PS, GS, HS and DS GetConstantBuffers entries.
    const GET_CONSTANTS: [usize; 5] = [72, 77, 81, 100, 104];
    let get: GetBuffers =
        unsafe { std::mem::transmute(slot(context, GET_CONSTANTS[index / STAGE_CONSTANTS])) };
    let mut buffer = null_mut();
    unsafe {
        get(context, (index % STAGE_CONSTANTS) as u32, 1, &mut buffer);
        ComRef::from_owned(buffer)
    }
}

fn begin_constant_override(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    indexed: bool,
    elements: u32,
) {
    render.constant_override.active = false;
    let callback: rsf_frame_tap_constant_override_fn = unsafe { load_fn(&TAP.constant_override) };
    if callback.is_none() || original(HOOK_VERTEX_CONSTANTS).is_null() {
        return;
    }
    if !render.geometry_valid {
        seed_geometry(platform, render, context);
    }
    let format = TAP.constant_override_format.load(Relaxed);
    let offered =
        render.target_texture.is_some() && format != 0 && format == render.target_facts.format;
    if !offered && !candidate_passes(render) {
        return;
    }
    let mut inputs = [ZERO_INPUT; MAX_INPUTS];
    let mut facts = divert_facts(render, indexed, elements, &mut inputs);
    stage_constants(platform, context, &mut facts);
    let mut slots = [0u32; CONSTANT_SLOTS];
    let mut buffers = [null_mut(); CONSTANT_SLOTS];
    let asked = unsafe {
        (platform.override_constants.unwrap())(
            callback,
            TAP.constant_override_user.load(Relaxed),
            &facts,
            slots.as_mut_ptr(),
            buffers.as_mut_ptr(),
        )
    };
    if asked <= 0 {
        return;
    }
    let state = &mut render.constant_override;
    state.count = 0;
    let mut used = [false; CONSTANT_SLOTS];
    for (&slot, &buffer) in slots.iter().zip(&buffers).take(asked as usize) {
        let index = slot as usize;
        if buffer.is_null() || index >= CONSTANT_SLOTS || used[index] {
            continue;
        }
        used[index] = true;
        state.slots[state.count] = slot;
        // Query the actual slot after the callback. The getter returns the reference
        // that keeps the original live through the replacement binding and draw.
        state.originals[state.count] = bound_constant(context, index);
        bind_override(platform, context, slot, buffer);
        state.count += 1;
    }
    if state.count == 0 {
        return;
    }
    state.active = true;
    bump(DRAWS_OVERRIDDEN);
}

fn end_constant_override(platform: &Platform, render: &mut Render, context: Handle) {
    let state = &mut render.constant_override;
    if !state.active {
        return;
    }
    let count = state.count;
    for (&slot, buffer) in state.slots[..count]
        .iter()
        .zip(&mut state.originals[..count])
    {
        let original = buffer.take();
        bind_override(
            platform,
            context,
            slot,
            original.as_ref().map_or(null_mut(), ComRef::as_ptr),
        );
    }
    state.count = 0;
    state.active = false;
}

fn spend(budget: &AtomicU32) -> bool {
    let mut current = budget.load(Relaxed);
    while current != 0 {
        if current == UNLIMITED {
            return true;
        }
        match budget.compare_exchange_weak(current, current - 1, Relaxed, Relaxed) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
    false
}

// Report this draw to whoever asked: a watched target, the input watch, the candidate prefilter
// or research. `before` offers it to the research callback ahead of the game's call.
fn consider_target_draw(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    indexed: bool,
    elements: u32,
    before: bool,
) {
    let research: rsf_frame_tap_target_fn = unsafe {
        load_fn(if before {
            &TAP.research_before
        } else {
            &TAP.research_draw
        })
    };
    let observed = observed(context);
    if before && (research.is_none() || !observed) {
        return;
    }
    let target = target_texture(render);
    if target.is_null() {
        return;
    }
    let ordinal = render.draws_into_target;
    if !before {
        render.draws_into_target = ordinal.wrapping_add(1);
    }
    let watch_index = TAP
        .watch
        .iter()
        .position(|watch| watch.load(Relaxed) == target)
        .unwrap_or(WATCH_SLOTS);
    if TAP.input_watch_dirty.load(Relaxed) {
        let watched = TAP.input_watch.load(Relaxed);
        let bound = !watched.is_null()
            && occupied(render).any(|index| slot_texture(render, index) == watched);
        render.input_watch_bound = bound;
        TAP.input_watch_dirty.store(false, Relaxed);
    }
    let options = options();
    let report_input =
        !before && options.on_input_draw.is_some() && render.input_watch_bound && observed;
    let report_candidate =
        !before && options.on_candidate_draw.is_some() && observed && candidate_passes(render);
    if report_candidate {
        bump(CANDIDATE_DRAWS);
    }
    let report_target = !before
        && watch_index != WATCH_SLOTS
        && options.on_target_draw.is_some()
        && spend(&TAP.watch_budget[watch_index]);
    let report_research = research.is_some() && observed;
    if !report_target && !report_input && !report_candidate && !report_research {
        return;
    }

    let mut inputs = [ZERO_INPUT; MAX_INPUTS];
    let mut report = common_facts(render, indexed, elements);
    report.context = context;
    report.watch_index = watch_index as u32;
    report.draw_index = ordinal;
    collect_inputs(render, &mut report, &mut inputs);
    // The one fact the shadow cannot supply, asked only on a reported draw.
    let mut viewport = ZERO_VIEWPORT;
    if unsafe { (platform.viewport.unwrap())(context, &mut viewport) } >= 1 {
        report.viewport_x = viewport.x;
        report.viewport_y = viewport.y;
        report.viewport_width = viewport.width as u32;
        report.viewport_height = viewport.height as u32;
    }

    // Counted whether or not the caller acts on it: a watch that never fires is what this
    // distinguishes.
    if report_target {
        bump(TARGET_DRAWS);
        unsafe {
            (platform.report.unwrap())(options.on_target_draw, options.on_target_draw_user, &report)
        };
    }
    if report_input {
        report.watch_index = WATCH_SLOTS as u32;
        unsafe {
            (platform.report.unwrap())(options.on_input_draw, options.on_input_draw_user, &report)
        };
    }
    if report_candidate {
        // Reported for what it is made of, not because anyone named its target.
        report.watch_index = WATCH_SLOTS as u32;
        unsafe {
            (platform.report.unwrap())(
                options.on_candidate_draw,
                options.on_candidate_draw_user,
                &report,
            )
        };
    }
    if report_research {
        if !render.geometry_valid {
            seed_geometry(platform, render, context);
        }
        report.vertex_shader = render.geometry.vertex_shader;
        report.input_layout = render.geometry.input_layout;
        report.vertex_stride = render.geometry.strides[0];
        report.topology = render.geometry.topology;
        report.vertex_constants = render.geometry.vertex_constants;
        report.pixel_constants = render.pixel_constants;
        stage_constants(platform, context, &mut report);
        unsafe { (platform.report.unwrap())(research, TAP.research_user.load(Relaxed), &report) };
    }
}

// Judged against the presented size with the render size unknown. resource_roles.rs owns the
// rules' meaning; the tap only holds the caller's copied table.
fn role_of(facts: &rsf_texture_facts) -> u32 {
    let setup = setup();
    let shape = rsf_frame_shape {
        struct_size: size_of::<rsf_frame_shape>() as u32,
        output_width: setup.options.output_width,
        output_height: setup.options.output_height,
        render_width: 0,
        render_height: 0,
    };
    let rules = &setup.rules[..(setup.rule_count as usize).min(ROLE_RULES)];
    matching_rule(rules, facts, &shape).map_or(0, |rule| rule.role)
}

// Whether anything reads the shader resource shadow. Without a reader, keeping it is a resource
// query per binding for nothing.
fn refresh_shadow_need() {
    let options = options();
    let needed = options.on_pass.is_some()
        || options.on_target_draw.is_some()
        || options.on_input_draw.is_some()
        || options.on_candidate_draw.is_some()
        || !TAP.research_draw.load(Relaxed).is_null()
        || !TAP.research_before.load(Relaxed).is_null()
        || TAP.divert.armed.load(Relaxed)
        || !TAP.constant_override.load(Relaxed).is_null();
    TAP.shadow_needed.store(needed, Relaxed);
}

// Put one view into the shadow. `resolved` is the texture behind it when the caller took it.
fn shadow_view(
    platform: &Platform,
    render: &mut Render,
    index: usize,
    view: Handle,
    resolved: Option<ComRef>,
) {
    drop_slot(render, index);
    if view.is_null() {
        return;
    }
    render.slots[index].view = view;
    set_occupied(render, index, true);
    // A buffer or a 3D texture keeps the slot occupied with no texture.
    let Some(texture) = resolved.or_else(|| texture_behind(platform, view)) else {
        return;
    };
    if texture.as_ptr() == target_texture(render) {
        // PSSetShaderResources refuses a read conflicting with the current output.
        drop_slot(render, index);
        return;
    }
    render.slots[index].facts = facts_of(platform, texture.as_ptr());
    render.slots[index].texture = Some(texture);
}

// Bring the shadow in line with whether anything reads it. A new shadow is seeded from the
// context, which has already unbound what it refused; one no longer read drops its references.
fn sync_shadow(platform: &Platform, render: &mut Render, context: Handle) {
    let wanted = TAP.shadow_needed.load(Relaxed);
    if wanted == render.shadow_live {
        return;
    }
    render.shadow_live = wanted;
    render.shadow_dirty = true;
    TAP.input_watch_dirty.store(true, Relaxed);
    if !wanted {
        for index in occupied(render) {
            drop_slot(render, index);
        }
        render.signature_complete = false;
        return;
    }
    let mut views = [null_mut(); VIEWS];
    unsafe { (platform.shader_views.unwrap())(context, views.as_mut_ptr()) };
    for (index, view) in views.into_iter().enumerate() {
        if let Some(view) = unsafe { ComRef::from_owned(view) } {
            shadow_view(platform, render, index, view.as_ptr(), None);
        }
    }
}

// Look at what is bound at the draw and, when it is the reconstruction input set, hand it over.
// Judged at the draw because the set arrives a slot at a time and is complete only when used.
fn consider_bound_set(platform: &Platform, render: &mut Render, context: Handle) {
    let setup = setup();
    let options = &setup.options;
    // Native owners use graph inputs; the shadow stays for captures.
    if options.on_pass.is_none() || !render.shadow_dirty {
        return;
    }
    render.shadow_dirty = false;
    if setup.rule_count == 0 {
        return;
    }

    // Judged against the presented size with the render size unknown, so the classifier accepts
    // anything from half of it upwards that keeps the frame's aspect.
    let (mut motion, mut depth, mut exposure, mut history) =
        (null_mut(), null_mut(), null_mut(), null_mut());
    let (mut motion_width, mut motion_height) = (0, 0);
    for index in occupied(render) {
        let entry = &mut render.slots[index];
        let Some(texture) = entry.texture.as_ref().map(ComRef::as_ptr) else {
            continue;
        };
        entry.role = role_of(&entry.facts);
        match entry.role {
            ROLE_MOTION if motion.is_null() => {
                motion = texture;
                motion_width = entry.facts.width;
                motion_height = entry.facts.height;
                bump(MOTION_SEEN);
            }
            ROLE_DEPTH if depth.is_null() => {
                depth = texture;
                bump(DEPTH_SEEN);
            }
            ROLE_EXPOSURE if exposure.is_null() => {
                exposure = texture;
                bump(EXPOSURE_SEEN);
            }
            // The remaining target with scene colour's shape: a candidate, not a demonstrated
            // history buffer.
            ROLE_HISTORY if history.is_null() => history = texture,
            _ => {}
        }
    }

    // Scene colour by its role and the motion target's size, never by slot: slot 0 of this set
    // holds GBuffer normals.
    let (mut scene, mut scene_format) = (null_mut(), 0);
    if !motion.is_null() {
        for index in occupied(render) {
            let entry = &render.slots[index];
            if let Some(texture) = &entry.texture
                && entry.role == ROLE_SCENE_COLOR
                && entry.facts.width == motion_width
                && entry.facts.height == motion_height
            {
                scene = texture.as_ptr();
                scene_format = entry.facts.format;
                break;
            }
        }
    }

    // A near miss, two of motion, depth and colour, says which combinations occur. Bounded.
    let present = [motion, depth, scene]
        .iter()
        .filter(|role| !role.is_null())
        .count();
    if present == 2 && render.described < NEAR_MISSES {
        render.described += 1;
        let described = render.described;
        let yes = |found: Handle| if found.is_null() { "no" } else { "yes" };
        say(platform, || {
            format!(
                "near miss {described}: motion {}, depth {}, colour {}, exposure {}. bound set follows",
                yes(motion),
                yes(depth),
                yes(scene),
                yes(exposure)
            )
        });
        for index in occupied(render) {
            let entry = &render.slots[index];
            if entry.texture.is_none() {
                continue;
            }
            let (facts, role) = (entry.facts, entry.role);
            say(platform, || {
                format!(
                    "  slot {index}: {}x{} format {} binds 0x{:x} mips {} samples {} role {role}",
                    facts.width,
                    facts.height,
                    facts.format,
                    facts.bind_flags,
                    facts.mip_levels,
                    facts.sample_count
                )
            });
        }
    }

    // Exposure is not part of the signature: the game binds it elsewhere. Edge triggered, so a
    // set that stays bound across draws runs a backend once.
    let qualifies = !motion.is_null() && !depth.is_null() && !scene.is_null();
    let was_complete = render.signature_complete;
    render.signature_complete = qualifies;
    if !qualifies || was_complete {
        return;
    }
    let frame_index = TAP.counters[PASSES].fetch_add(1, Relaxed).wrapping_add(1);
    let last_width = TAP.counters[RENDER_WIDTH].swap(motion_width, Relaxed);
    let last_height = TAP.counters[RENDER_HEIGHT].swap(motion_height, Relaxed);
    // AddRef under the lock, because a concurrent uninstall would otherwise release the last
    // reference between the read and the retain.
    let (live, constants) = {
        let lock = held();
        (TAP.installed.load(Relaxed), lock.0.clone())
    };
    let mut pass: rsf_frame_tap_pass = unsafe { zeroed() };
    pass.struct_size = size_of::<rsf_frame_tap_pass>() as u32;
    pass.context = context;
    pass.scene_color = scene;
    pass.history = history;
    pass.motion = motion;
    pass.depth = depth;
    pass.exposure = exposure;
    pass.view_constants = constants.as_ref().map_or(null_mut(), ComRef::as_ptr);
    pass.render_width = motion_width;
    pass.render_height = motion_height;
    pass.frame_index = frame_index;
    pass.scene_color_format = scene_format;
    // Announced before the callback, so a callback that ends the process leaves a record. Only
    // the first pass and a change of render size: this is mid-frame on the render thread.
    if live && (frame_index == 1 || last_width != motion_width || last_height != motion_height) {
        say(platform, || {
            format!(
                "reconstruction input set {frame_index} at {motion_width}x{motion_height}, {} \
                 history, {} view constants",
                if history.is_null() { "no" } else { "with" },
                if constants.is_some() { "with" } else { "no" }
            )
        });
    }
    // No lock across the callback: it calls back into D3D. `live` can be stale, which narrows the
    // window in which a detached caller is still called rather than closing it.
    if live {
        unsafe { (platform.pass.unwrap())(options.on_pass, options.on_pass_user, &pass) };
    }
}

// Seed inherited IA/VS/PS constant state on the render thread, also after ClearState or a command
// list. The context owns the live bindings.
fn seed_geometry(platform: &Platform, render: &mut Render, context: Handle) {
    unsafe {
        (platform.seed_geometry.unwrap())(
            context,
            &mut render.geometry,
            render.pixel_constants.as_mut_ptr(),
        )
    };
    // ID3D11DeviceContext::PSGetShader, OMGetBlendState and OMGetDepthStencilState.
    // Like the IA/VS queries above, these only seed handles the context currently owns.
    type GetShader = unsafe extern "system" fn(Handle, *mut Handle, *mut Handle, *mut u32);
    type GetBlend = unsafe extern "system" fn(Handle, *mut Handle, *mut f32, *mut u32);
    type GetDepth = unsafe extern "system" fn(Handle, *mut Handle, *mut u32);
    unsafe {
        let get_shader: GetShader = std::mem::transmute(slot(context, 74));
        let get_blend: GetBlend = std::mem::transmute(slot(context, 91));
        let get_depth: GetDepth = std::mem::transmute(slot(context, 92));
        get_shader(context, &mut render.pixel_shader, null_mut(), null_mut());
        get_blend(
            context,
            &mut render.blend_state,
            [0.0; 4].as_mut_ptr(),
            &mut 0,
        );
        get_depth(context, &mut render.depth_stencil_state, &mut 0);
        for object in [
            render.pixel_shader,
            render.blend_state,
            render.depth_stencil_state,
        ] {
            drop(ComRef::from_owned(object));
        }
    }
    render.geometry_valid = true;
}

// Replay must happen here: retaining a buffer does not preserve its contents across uploads.
fn report_geometry(
    platform: &Platform,
    render: &mut Render,
    context: Handle,
    call: &rsf_frame_tap_draw_call,
    indirect: Handle,
    offset: u32,
) {
    let options = options();
    let target = target_texture(render);
    if options.on_geometry.is_none()
        || render.geometry_depth.is_null()
        || render.target_count != 1
        || target.is_null()
        || TAP.target_substituted.load(Relaxed)
    {
        return;
    }
    if !render.geometry_valid {
        seed_geometry(platform, render, context);
    }
    let facts = render.target_facts;
    let geometry = &mut render.geometry;
    geometry.context = context;
    geometry.target = target;
    geometry.depth_view = render.geometry_depth;
    geometry.width = facts.width;
    geometry.height = facts.height;
    geometry.format = facts.format;
    geometry.samples = facts.sample_count;
    geometry.kind = call.kind;
    geometry.count = call.count;
    geometry.start = call.start;
    geometry.base_vertex = call.base_vertex;
    geometry.instances = call.instances;
    geometry.start_instance = call.first_instance;
    geometry.indirect_buffer = indirect;
    geometry.indirect_offset = offset;
    unsafe {
        (platform.geometry.unwrap())(options.on_geometry, options.on_geometry_user, geometry)
    };
}

// Command lists and ClearState invalidate all observation until the engine binds again.
fn invalidate_geometry(render: &mut Render) {
    render.geometry_valid = false;
    render.geometry = ZERO_GEOMETRY;
    render.pixel_shader = null_mut();
    render.blend_state = null_mut();
    render.depth_stencil_state = null_mut();
    render.pixel_constants = [null_mut(); STAGE_CONSTANTS];
    render.geometry_depth = null_mut();
    render.depth_bound = false;
    render.target_count = 0;
    TAP.target_substituted.store(false, Relaxed);
    render.game_viewport_count = 0;
    render.game_scissor_count = 0;
    let constants = {
        let mut lock = held();
        TAP.view_constants_hint.store(null_mut(), Relaxed);
        lock.0.take()
    };
    drop(constants);
    for index in occupied(render) {
        drop_slot(render, index);
    }
    render.shadow_dirty = true;
    render.signature_complete = false;
    TAP.input_watch_dirty.store(true, Relaxed);
    forget_target(render);
}

// Everything the tap holds of the game's: the patched blends, which are keyed by the game's
// pointers, the view constants, the shadowed bindings and the render target. Run once per
// uninstall, when no hook holds an admission any longer.
fn retire_render() {
    let _retiring = retiring();
    if !RETIRE_PENDING.load(SeqCst) {
        return;
    }
    let render = unsafe { render() };
    pending().fill(None);
    render.patched_blends.clear();
    invalidate_geometry(render);
    render.shadow_dirty = false;
    render.shadow_live = false;
    render.input_watch_bound = false;
    RETIRE_PENDING.store(false, SeqCst);
}

// Whether `resource` is a constant buffer the watch wants, and its width. Only a "no" is
// remembered: a width becomes a memory size the caller copies, and a recycled address must not be
// taken for it.
#[derive(Clone, Copy)]
enum ConstantCallback {
    Observer(rsf_frame_tap_constant_observer_fn),
    Legacy(rsf_frame_tap_constants_fn, bool),
}

#[derive(Clone, Copy)]
struct ConstantWatch {
    callback: ConstantCallback,
    user: Handle,
    bytes: u32,
}

// The callback user address is opaque to the tap and used under a hook admission.
unsafe impl Send for ConstantWatch {}

static CONSTANT_WATCH: Mutex<Option<ConstantWatch>> = Mutex::new(None);

fn constant_watch() -> MutexGuard<'static, Option<ConstantWatch>> {
    CONSTANT_WATCH
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

impl ConstantWatch {
    unsafe fn notify(self, platform: &Platform, resource: Handle, data: *const c_void, bytes: u32) {
        match self.callback {
            ConstantCallback::Observer(callback) => unsafe {
                callback.unwrap()(self.user, resource, data, bytes)
            },
            ConstantCallback::Legacy(callback, _) => unsafe {
                (platform.constants.unwrap())(callback, self.user, resource, data.cast_mut(), bytes)
            },
        }
    }
}

fn watched_buffer(platform: &Platform, render: &mut Render, resource: Handle, watched: u32) -> u32 {
    let epoch = TAP.verdict_epoch.load(Relaxed);
    let expired = epoch != render.verdict_epoch_seen || {
        render.verdict_lookups += 1;
        render.verdict_lookups >= VERDICT_LIFETIME
    };
    if expired {
        render.verdict_epoch_seen = epoch;
        render.verdict_lookups = 0;
        render.verdicts = [(null_mut(), 0); VERDICT_SLOTS];
    }
    let index = ((resource as usize) >> 4) % VERDICT_SLOTS;
    if render.verdicts[index] == (resource, 0) {
        return 0;
    }
    let mut bind_flags = 0;
    let width = unsafe { (platform.buffer_width.unwrap())(resource, &mut bind_flags) };
    let wanted = if watched == 0 {
        width <= 4096
    } else {
        width == watched
    };
    let bytes = if width != 0 && wanted && bind_flags & BIND_CONSTANT_BUFFER != 0 {
        width
    } else {
        0
    };
    render.verdicts[index] = (resource, bytes);
    bytes
}

fn keep_view_constants(buffer: Handle) {
    let kept = unsafe { ComRef::retain(buffer) };
    let released = {
        let mut lock = held();
        // A call in flight after uninstall must not hand over a reference nothing would release.
        if TAP.installed.load(Relaxed) {
            TAP.view_constants_hint.store(buffer, Relaxed);
            lock.0.replace(kept)
        } else {
            Some(kept)
        }
    };
    // A final release runs the object's destructor, so it happens outside the lock.
    drop(released);
}

// Textures resolved for a substitution, owned until the shadow adopts them.
struct Resolved {
    handles: [MaybeUninit<Handle>; VIEWS],
    count: usize,
}

impl Resolved {
    fn take(&mut self, index: usize) -> Option<ComRef> {
        if index >= self.count {
            return None;
        }
        let handle = std::mem::replace(&mut self.handles[index], MaybeUninit::new(null_mut()));
        unsafe { ComRef::from_owned(handle.assume_init()) }
    }
}

impl Drop for Resolved {
    fn drop(&mut self) {
        for index in 0..self.count {
            drop(self.take(index));
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_install(
    platform: &Platform,
    context: Handle,
    options: *const rsf_frame_tap_options,
    hooks: *const rsf_frame_tap_hook,
) -> i32 {
    if context.is_null()
        || options.is_null()
        || unsafe { (*options).struct_size } < size_of::<rsf_frame_tap_options>() as u32
    {
        return INVALID;
    }
    let options = unsafe { &*options };
    if options.abi_version != RSF_FRAME_TAP_ABI_VERSION {
        return ABI_MISMATCH;
    }
    let roles = unsafe { options.roles.as_ref() };
    // Refused rather than truncated: a table cut short is a role that stops being recognised.
    if let Some(roles) = roles
        && (roles.struct_size < size_of::<rsf_role_table>() as u32
            || roles.count > RSF_FRAME_TAP_MAX_ROLE_RULES
            || (roles.count != 0 && roles.rules.is_null()))
    {
        return INVALID;
    }

    // A caller inside a hook belongs to a session that is installed or still retiring, so a busy
    // lock is the same answer as the check below.
    let Some(lock) = control() else {
        return ALREADY_INSTALLED;
    };
    let retiring = retiring();
    // A session retired from inside one of its own callbacks still owns the shadow until that
    // hook returns.
    if TAP.installed.load(SeqCst)
        || RETIRE_PENDING.load(SeqCst)
        || !HOOK_TABLE.vtable.load(Acquire).is_null()
    {
        return ALREADY_INSTALLED;
    }
    let setup = unsafe { &mut *TAP.setup.get() };
    setup.options = *options;
    setup.options.roles = null();
    setup.rule_count = 0;
    if let Some(roles) = roles
        && roles.count != 0
    {
        let count = roles.count as usize;
        setup.rules[..count].copy_from_slice(unsafe { slice::from_raw_parts(roles.rules, count) });
        setup.rule_count = roles.count;
    }
    TAP.context.store(context, Relaxed);
    if setup.options.view_constant_bytes == 0 {
        setup.options.view_constant_bytes = 4096;
    }

    // Keep the context alive until all writes back to its table have succeeded.
    let vtable = unsafe { *context.cast::<*mut Handle>() };
    unsafe { *HOOK_TABLE.context.get() = Some(ComRef::retain(context)) };
    HOOK_TABLE.vtable.store(vtable, Relaxed);
    let hooks = unsafe { slice::from_raw_parts(hooks, HOOKS) };
    HOOK_TABLE
        .sentinel
        .store(hooks[HOOK_DRAW].replacement, Relaxed);
    let context1 = unsafe { (platform.context1.unwrap())(context) } != 0;
    // Every slot at once, undone as a unit: bindings and draws only mean anything together.
    for (hook, entry) in hooks.iter().enumerate() {
        HOOK_TABLE.replacements[hook].store(null_mut(), Relaxed);
        if entry.context1 != 0 && !context1 {
            continue;
        }
        HOOK_TABLE.slots[hook].store(entry.slot as usize, Relaxed);
        if patch(platform, vtable, hook, entry.replacement) {
            HOOK_TABLE.replacements[hook].store(entry.replacement, Release);
            continue;
        }
        restore(platform, vtable, (0..hook).rev());
        return PATCH_FAILED;
    }

    HOOK_TABLE.refreshes.store(0, Relaxed);
    unsafe { render() }.shadow_live = false;
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    TAP.installed.store(true, SeqCst);
    refresh_shadow_need();
    drop(retiring);
    drop(lock);
    if options.on_pass.is_some() && roles.is_none() {
        say(platform, || {
            "frame tap: a pass callback and no role table, so no input set will be recognised"
                .to_owned()
        });
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_uninstall(platform: &Platform) -> i32 {
    // Everything that can refuse comes before the first change. A caller inside a hook is refused
    // with INVALID_ARGUMENT when it would have to wait for another thread.
    let Some(lock) = control() else {
        return INVALID;
    };
    let vtable = HOOK_TABLE.vtable.load(Acquire);
    if vtable.is_null() {
        return NOT_INSTALLED;
    }
    // A previous failed restore left forwarding-only hooks that still need to be removed.
    if !TAP.installed.load(SeqCst) {
        return if restore(platform, vtable, 0..HOOKS) {
            OK
        } else {
            PATCH_FAILED
        };
    }
    // No hook on another thread is admitted from here on: none is using the shadow, a substitute
    // or a biased clone, and none starts until the gate reopens on a session that is gone.
    let Some(_writer) = drain(lock) else {
        return INVALID;
    };
    TAP.research_draw.store(null_mut(), Release);
    TAP.research_before.store(null_mut(), Release);
    TAP.research_compute.store(null_mut(), Release);
    TAP.plan_active.store(false, SeqCst);
    TAP.divert.armed.store(false, Relaxed);
    TAP.constant_override.store(null_mut(), Relaxed);
    TAP.constant_override_format.store(0, Relaxed);
    *constant_watch() = None;
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    TAP.shadow_needed.store(false, Relaxed);
    // Off before the table goes back, so a hook in flight cannot re-apply it or be admitted. Each
    // slot gets whatever the runtime last had there.
    TAP.installed.store(false, SeqCst);
    let restored = restore(platform, vtable, 0..HOOKS);
    TAP.divert.layer_target.store(null_mut(), Relaxed);
    TAP.divert.layer_texture.store(null_mut(), Relaxed);
    TAP.divert.verdict.store(null_mut(), Relaxed);
    RETIRE_PENDING.store(true, SeqCst);
    {
        let mut samplers = sampler_state();
        samplers.biased.clear();
        samplers.clones.clear();
        samplers.bias = 0.0;
        TAP.sampler_bias_active.store(false, Release);
    }
    TAP.input_watch.store(null_mut(), Relaxed);
    TAP.context.store(null_mut(), Relaxed);
    // The watches name the caller's textures; a later install must not match a past session's.
    // Under the lock rsf_frame_tap_watch_target checks `installed` with.
    {
        let _held = held();
        for (watch, budget) in TAP.watch.iter().zip(&TAP.watch_budget) {
            watch.store(null_mut(), Relaxed);
            budget.store(0, Relaxed);
        }
    }
    unsafe {
        *TAP.plan.get() = ZERO_PLAN;
        (*TAP.plan_views.get()).clear();
    }
    TAP.plan_generation.fetch_add(1, SeqCst);
    close_gates();
    // From inside a callback this thread's own hook is still on the stack and retires the shadow
    // when its admission ends.
    if !admitted_here() {
        retire_render();
    }
    if restored { OK } else { PATCH_FAILED }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_refresh(platform: &Platform) -> i32 {
    if !TAP.installed.load(Acquire) {
        return NOT_INSTALLED;
    }
    refresh(platform);
    // Once a frame from the present hook, when resource address lookups stop being trusted.
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_set_sampler_bias(platform: &Platform, bias: f32) -> i32 {
    if !bias.is_finite() || !(-16.0..=0.0).contains(&bias) {
        return INVALID;
    }
    let context = TAP.context.load(Relaxed);
    let forward = original(HOOK_SAMPLERS);
    if !TAP.installed.load(Acquire) || context.is_null() || forward.is_null() {
        return NOT_INSTALLED;
    }
    let mut state = sampler_state();
    if state.bias == bias {
        return OK;
    }
    state.bias = bias;
    // Unreal's state cache skips a sampler it believes bound, so the live bindings are rewritten
    // here. Every slot is resolved to its original before any is re-biased: re-biasing drops the
    // old clone's mapping, and a later slot holding it would be biased twice.
    let mut current = [null_mut(); SAMPLERS];
    unsafe { (platform.samplers.unwrap())(context, current.as_mut_ptr()) };
    let current = current.map(|sampler| unsafe { ComRef::from_owned(sampler) });
    let originals = current
        .each_ref()
        .map(|sampler| state.original_of(sampler.as_ref().map_or(null_mut(), ComRef::as_ptr)));
    let bound = originals.map(|sampler| state.biased(platform, context, sampler));
    unsafe {
        (platform.set_samplers.unwrap())(forward, context, 0, SAMPLERS as u32, bound.as_ptr())
    };
    drop(current);
    TAP.sampler_bias_active.store(bias != 0.0, Release);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_tap_set_plan(plan: *const rsf_frame_tap_plan) -> i32 {
    // Everything that can refuse comes before the first change, so a refused call keeps the old
    // plan. A caller inside a hook is refused with INVALID_ARGUMENT when it would have to wait for
    // another thread.
    let Some(lock) = control() else {
        return INVALID;
    };
    if !TAP.installed.load(SeqCst) {
        return NOT_INSTALLED;
    }
    let replacement = if plan.is_null() {
        None
    } else {
        if unsafe { (*plan).struct_size } < size_of::<rsf_frame_tap_plan>() as u32 {
            return INVALID;
        }
        let plan = unsafe { &*plan };
        if plan.count == 0
            || plan.count > RSF_FRAME_TAP_MAX_SUBSTITUTIONS
            || plan.viewport_scale_x <= 0.0
            || plan.viewport_scale_y <= 0.0
        {
            return INVALID;
        }
        // A named texture with nothing in its place reads like an intervention and does nothing.
        if plan.items[..plan.count as usize].iter().any(|item| {
            item.texture.is_null() || (item.shader_view.is_null() && item.render_view.is_null())
        }) {
            return INVALID;
        }
        Some(*plan)
    };
    // Every hook that saw the old plan on another thread has forwarded and left, and none starts
    // until the gate reopens, so nothing reads the plan while it is overwritten and the caller may
    // release the old views when this returns.
    let Some(_writer) = drain(lock) else {
        return INVALID;
    };
    let mut views = Vec::new();
    if let Some(plan) = &replacement {
        for item in &plan.items[..plan.count as usize] {
            for object in [
                item.texture,
                item.shader_view,
                item.render_view,
                item.after_target,
            ] {
                if !object.is_null() {
                    views.push(unsafe { ComRef::retain(object) });
                }
            }
        }
    }
    TAP.plan_active.store(false, SeqCst);
    unsafe {
        *TAP.plan.get() = replacement.unwrap_or(ZERO_PLAN);
        *TAP.plan_views.get() = views;
    }
    TAP.plan_generation.fetch_add(1, SeqCst);
    close_gates();
    TAP.target_substituted.store(false, Relaxed);
    if replacement.is_some() {
        TAP.plan_active.store(true, SeqCst);
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_end_frame() -> i32 {
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    // No lock: the present hook runs on the render thread, which is where the gates open.
    if TAP.plan_active.load(Acquire) {
        close_gates();
    }
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_watch_input(texture: Handle) -> i32 {
    if !TAP.installed.load(Acquire) {
        return NOT_INSTALLED;
    }
    TAP.input_watch.store(texture, Relaxed);
    TAP.input_watch_dirty.store(true, Relaxed);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_watch(
    bytes: u32,
    watch: rsf_frame_tap_constants_fn,
    user: Handle,
) -> i32 {
    *constant_watch() = watch.map(|callback| ConstantWatch {
        callback: ConstantCallback::Legacy(Some(callback), true),
        user,
        bytes,
    });
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_observer(
    bytes: u32,
    callback: rsf_frame_tap_constant_observer_fn,
    user: Handle,
) -> i32 {
    *constant_watch() = callback.map(|callback| ConstantWatch {
        callback: ConstantCallback::Observer(Some(callback)),
        user,
        bytes,
    });
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_watch_writable(writable: u32) {
    if let Some(watch) = constant_watch().as_mut()
        && let ConstantCallback::Legacy(_, mode) = &mut watch.callback
    {
        *mode = writable != 0;
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_research_callbacks(
    draw: rsf_frame_tap_target_fn,
    compute: rsf_frame_tap_compute_fn,
    user: Handle,
) -> i32 {
    rsf_frame_tap_set_research_phase_callbacks(None, draw, compute, user)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_research_phase_callbacks(
    before: rsf_frame_tap_target_fn,
    after: rsf_frame_tap_target_fn,
    compute: rsf_frame_tap_compute_fn,
    user: Handle,
) -> i32 {
    if !TAP.installed.load(Acquire) {
        return NOT_INSTALLED;
    }
    TAP.research_draw.store(null_mut(), Release);
    TAP.research_before.store(null_mut(), Release);
    TAP.research_compute.store(null_mut(), Release);
    TAP.research_user.store(user, Relaxed);
    unsafe {
        store_fn(&TAP.research_draw, after);
        store_fn(&TAP.research_before, before);
        store_fn(&TAP.research_compute, compute);
    }
    refresh_shadow_need();
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_bound_target() -> Handle {
    TAP.bound_target.load(Relaxed)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_override(
    callback: rsf_frame_tap_constant_override_fn,
    user: Handle,
) -> i32 {
    TAP.constant_override_user.store(user, Relaxed);
    unsafe { store_fn(&TAP.constant_override, callback) };
    refresh_shadow_need();
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_override_format(format: u32) {
    TAP.constant_override_format.store(format, Release);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_set_divert(
    platform: &Platform,
    setup: *const rsf_frame_tap_divert_setup,
) -> i32 {
    let divert = &TAP.divert;
    let setup = unsafe { setup.as_ref() };
    let Some(setup) =
        setup.filter(|setup| !setup.layer_target.is_null() && setup.verdict.is_some())
    else {
        // Disarmed before the layer is dropped, so a draw in flight cannot find it going away.
        divert.armed.store(false, Release);
        divert.layer_target.store(null_mut(), Relaxed);
        divert.layer_texture.store(null_mut(), Relaxed);
        divert.verdict.store(null_mut(), Relaxed);
        divert.verdict_user.store(null_mut(), Relaxed);
        refresh_shadow_need();
        return OK;
    };
    if setup.struct_size < size_of::<rsf_frame_tap_divert_setup>() as u32 {
        return INVALID;
    }
    // Borrowed: the layer owns its texture and outlives the setting.
    let texture =
        texture_behind(platform, setup.layer_target).map_or(null_mut(), |texture| texture.as_ptr());
    divert.layer_target.store(setup.layer_target, Relaxed);
    divert.layer_texture.store(texture, Relaxed);
    divert.layer_width.store(setup.layer_width, Relaxed);
    divert.layer_height.store(setup.layer_height, Relaxed);
    unsafe { store_fn(&divert.verdict, setup.verdict) };
    divert.verdict_user.store(setup.verdict_user, Relaxed);
    divert.armed.store(true, Release);
    refresh_shadow_need();
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_tap_set_candidates(
    candidates: *const rsf_frame_tap_candidates,
) -> i32 {
    let sets = &TAP.candidates;
    let all = [&sets.layouts, &sets.widget_targets, &sets.shaders];
    if candidates.is_null() {
        // Counts first, so a draw in flight stops looking before the entries are touched.
        sets.armed.store(false, Relaxed);
        for set in all {
            set.count.store(0, Release);
        }
        return OK;
    }
    if unsafe { (*candidates).struct_size } < size_of::<rsf_frame_tap_candidates>() as u32 {
        return INVALID;
    }
    let candidates = unsafe { &*candidates };
    let limit = CANDIDATES as u32;
    // Refused rather than truncated: a shortened set stops matching partway down.
    if candidates.layout_count > limit
        || candidates.widget_target_count > limit
        || candidates.shader_count > limit
    {
        return INVALID;
    }
    let sources = [
        (candidates.layouts, candidates.layout_count),
        (candidates.widget_targets, candidates.widget_target_count),
        (candidates.shaders, candidates.shader_count),
    ];
    let mut total = 0;
    for (set, (source, count)) in all.into_iter().zip(sources) {
        let count = if source.is_null() { 0 } else { count };
        // Shrink, write, then publish: a reader sees fewer entries, never one being overwritten.
        set.count.store(0, Release);
        if count != 0 {
            let entries = unsafe { slice::from_raw_parts(source, count as usize) };
            for (slot, &entry) in set.entries.iter().zip(entries) {
                slot.store(entry, Relaxed);
            }
        }
        set.count.store(count, Release);
        total += count;
    }
    sets.armed.store(total != 0, Relaxed);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_watch_target(index: u32, texture: Handle, limit: u32) -> i32 {
    let index = index as usize;
    if index >= WATCH_SLOTS {
        return INVALID;
    }
    let _lock = held();
    if !TAP.installed.load(Relaxed) {
        return NOT_INSTALLED;
    }
    // The budget first, so the render thread cannot spend the previous watch's budget.
    let budget = if texture.is_null() {
        0
    } else if limit == 0 {
        UNLIMITED
    } else {
        limit
    };
    TAP.watch_budget[index].store(budget, Relaxed);
    TAP.watch[index].store(texture, Relaxed);
    OK
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_frame_tap_get_status(status: *mut rsf_frame_tap_status) -> i32 {
    if status.is_null()
        || unsafe { (*status).struct_size } < size_of::<rsf_frame_tap_status>() as u32
    {
        return INVALID;
    }
    let status = unsafe { &mut *status };
    let _lock = held();
    status.installed = u32::from(TAP.installed.load(Relaxed));
    status.calls_seen = counter(CALLS_SEEN);
    status.calls_inspected = counter(CALLS_INSPECTED);
    status.motion_seen = counter(MOTION_SEEN);
    status.depth_seen = counter(DEPTH_SEEN);
    status.exposure_seen = counter(EXPOSURE_SEEN);
    status.passes_seen = counter(PASSES);
    status.render_width = counter(RENDER_WIDTH);
    status.render_height = counter(RENDER_HEIGHT);
    status.target_draws_reported = counter(TARGET_DRAWS);
    status.plan_set = u32::from(TAP.plan_active.load(Relaxed));
    status.inputs_substituted = counter(INPUTS_SUBSTITUTED);
    status.targets_redirected = counter(TARGETS_REDIRECTED);
    status.gates_opened = counter(GATES_OPENED);
    status.depth_mismatches = counter(DEPTH_MISMATCHES);
    status.depth_mismatch_target_width = counter(MISMATCH_TARGET_WIDTH);
    status.depth_mismatch_target_height = counter(MISMATCH_TARGET_HEIGHT);
    status.depth_mismatch_depth_width = counter(MISMATCH_DEPTH_WIDTH);
    status.depth_mismatch_depth_height = counter(MISMATCH_DEPTH_HEIGHT);
    status.depth_mismatch_depth_format = counter(MISMATCH_DEPTH_FORMAT);
    status.candidate_draws = counter(CANDIDATE_DRAWS);
    status.draws_diverted = counter(DRAWS_DIVERTED);
    status.blend_states_patched = counter(BLENDS_PATCHED);
    status.divert_refused = counter(DIVERT_REFUSED);
    status.divert_last_refusal = counter(LAST_REFUSAL);
    status.vtable_refreshes = HOOK_TABLE.refreshes.load(Relaxed);
    status.draws_overridden = counter(DRAWS_OVERRIDDEN);
    status.copies_redirected = counter(COPIES_REDIRECTED);
    status.copies_mismatched = counter(COPIES_MISMATCHED);
    status.updates_watched = counter(UPDATES_WATCHED);
    status.gates_declined = counter(GATES_DECLINED);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_enter(platform: &Platform, hook: u32) -> Handle {
    refresh(platform);
    original(hook as usize)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_leave(platform: &Platform, admitted: u32) {
    refresh(platform);
    if admitted != 0 {
        // Ends the admission a deciding entry carried across the detour's forward.
        drop(Admission);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_views(
    platform: &Platform,
    context: Handle,
    start: u32,
    count: u32,
    views: *const Handle,
) {
    refresh(platform);
    let forward = original(HOOK_VIEWS);
    if forward.is_null() {
        // Dropping a binding is wrong and will show; a null call ends the process silently.
        return;
    }
    // Held through the forward, so a substitute cannot be released by a clear in between.
    let admission = Admission::enter(context);
    let admitted = admission.is_some();
    // A binding named by the plan is altered before it goes through. Only the views are swapped:
    // a pass that reads slot 3 has a shader that says slot 3.
    let mut substituted = [MaybeUninit::<Handle>::uninit(); VIEWS];
    let mut resolved = Resolved {
        handles: [MaybeUninit::uninit(); VIEWS],
        count: 0,
    };
    let mut forwarded = views;
    if !inside()
        && admitted
        && !views.is_null()
        && count > 0
        && count as usize <= VIEWS
        && TAP.plan_active.load(SeqCst)
    {
        let _guard = Reentry::enter();
        let render = unsafe { render() };
        let incoming = unsafe { slice::from_raw_parts(views, count as usize) };
        let mut any = false;
        for (index, &view) in incoming.iter().enumerate() {
            substituted[index] = MaybeUninit::new(view);
            resolved.handles[index] = MaybeUninit::new(null_mut());
            resolved.count = index + 1;
            if view.is_null() {
                continue;
            }
            // A view already in its slot has its texture there; any other is resolved once and
            // kept for the shadow to adopt.
            let known = render
                .slots
                .get(start as usize + index)
                .filter(|slot| slot.view == view)
                .and_then(|slot| slot.texture.as_ref())
                .map(ComRef::as_ptr);
            let texture = known.unwrap_or_else(|| {
                let texture = unsafe { (platform.texture_behind.unwrap())(view) };
                resolved.handles[index] = MaybeUninit::new(texture);
                texture
            });
            let replacement = applicable(texture, View::Shader);
            if replacement.is_null() {
                continue;
            }
            substituted[index] = MaybeUninit::new(replacement);
            any = true;
        }
        if any {
            forwarded = substituted.as_ptr().cast();
            bump(INPUTS_SUBSTITUTED);
        }
    }
    // Forwarded before the shadow, so it is in place before a callback binds something else.
    unsafe { (platform.set_shader_views.unwrap())(forward, context, start, count, forwarded) };

    if inside() || !admitted {
        return;
    }
    let _guard = Reentry::enter();
    bump(CALLS_SEEN);
    let render = unsafe { render() };
    sync_shadow(platform, render, context);
    // Only a slot whose view changed pays for a resource query.
    let mut changed = false;
    for index in 0..count as usize {
        let slot = start as usize + index;
        if !render.shadow_live || slot >= VIEWS {
            break;
        }
        let view = if views.is_null() {
            null_mut()
        } else {
            unsafe { *views.add(index) }
        };
        if render.slots[slot].view == view {
            continue;
        }
        changed = true;
        let adopted = resolved.take(index);
        shadow_view(platform, render, slot, view, adopted);
    }
    if !changed {
        return;
    }
    bump(CALLS_INSPECTED);
    render.shadow_dirty = true;
    TAP.input_watch_dirty.store(true, Relaxed);
}

// Decide what to bind at render target slot 0, opening any gate the binding opens. Only slot 0 of
// a single target binding is replaced: a multiple target pass whose first target moved to another
// resolution draws nothing at all.
unsafe fn plan_targets(
    platform: &Platform,
    context: Handle,
    count: u32,
    views: *const Handle,
    replacement: &mut Handle,
    depth: &mut Handle,
    resolved: &mut Option<ComRef>,
) -> *const Handle {
    if inside() || views.is_null() || count != 1 || !TAP.plan_active.load(SeqCst) {
        return views;
    }
    let _guard = Reentry::enter();
    let view = unsafe { *views };
    if view.is_null() {
        return views;
    }
    let Some(texture) = texture_behind(platform, view) else {
        return views;
    };
    let texture = resolved.insert(texture).as_ptr();
    // Before the substitution and before the binding is forwarded: a gate is the point in the
    // frame this binding marks. A gate callback that replaced the plan leaves this binding alone.
    if !open_gates_for(platform, context, texture) {
        return views;
    }
    let target = applicable(texture, View::Render);
    if target.is_null() {
        return views;
    }
    // The promoted target is at output resolution while the game's depth is not, and D3D11 rejects
    // that pair. The plan's depth policy chooses which loss to take.
    if !depth.is_null()
        && let Some(depth_facts) = view_extent(platform, *depth)
        && let Some(target_facts) = view_extent(platform, target)
        && (depth_facts.width != target_facts.width || depth_facts.height != target_facts.height)
    {
        bump(DEPTH_MISMATCHES);
        record(MISMATCH_TARGET_WIDTH, target_facts.width);
        record(MISMATCH_TARGET_HEIGHT, target_facts.height);
        record(MISMATCH_DEPTH_WIDTH, depth_facts.width);
        record(MISMATCH_DEPTH_HEIGHT, depth_facts.height);
        record(MISMATCH_DEPTH_FORMAT, depth_facts.format);
        let policy = plan().depth_policy;
        if policy == RSF_FRAME_TAP_DEPTH_REFUSE {
            return views;
        }
        if policy != RSF_FRAME_TAP_DEPTH_KEEP {
            *depth = null_mut();
        }
    }
    *replacement = target;
    bump(TARGETS_REDIRECTED);
    replacement
}

// Whether the bound target was replaced, and the viewport put where that needs it. After the
// binding is forwarded, so the viewport lands on the target it belongs to.
fn settle_target_substitution(
    platform: &Platform,
    render: &Render,
    context: Handle,
    substituted: bool,
) {
    if substituted == TAP.target_substituted.load(Relaxed) {
        return;
    }
    TAP.target_substituted.store(substituted, Relaxed);
    apply_viewport_policy(platform, render, context);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_targets(
    platform: &Platform,
    context: Handle,
    count: u32,
    views: *const Handle,
    depth: Handle,
    uavs: *const rsf_frame_tap_uavs,
) {
    refresh(platform);
    let hook = if uavs.is_null() {
        HOOK_TARGETS
    } else {
        HOOK_TARGETS_AND_UAVS
    };
    let forward = original(hook);
    if forward.is_null() {
        return;
    }
    let admission = Admission::enter(context);
    let admitted = admission.is_some();
    // KEEP leaves the render targets alone, so the shadow and the substitution leave them too.
    let keeps = !uavs.is_null() && count == KEEP_TARGETS;
    let mut replacement = null_mut();
    let mut forwarded_depth = depth;
    let mut resolved = None;
    let forwarded = if keeps || !admitted {
        views
    } else {
        unsafe {
            plan_targets(
                platform,
                context,
                count,
                views,
                &mut replacement,
                &mut forwarded_depth,
                &mut resolved,
            )
        }
    };
    unsafe {
        (platform.set_targets.unwrap())(forward, context, count, forwarded, forwarded_depth, uavs)
    };
    if inside() || !admitted || keeps {
        return;
    }
    // The shadow records what is bound now, not what the game asked for: geometry replay reads it
    // back, and a dropped depth is not there to replay against.
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    render.depth_bound = !forwarded_depth.is_null();
    render.geometry_depth = forwarded_depth;
    render.target_count = count;
    let first = if views.is_null() || count == 0 {
        null_mut()
    } else {
        unsafe { *views }
    };
    shadow_target(platform, render, first, resolved);
    unbind_target_reads(render);
    settle_target_substitution(platform, render, context, !replacement.is_null());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_viewports(
    platform: &Platform,
    context: Handle,
    count: u32,
    viewports: *const rsf_frame_tap_viewport,
) {
    refresh(platform);
    let forward = original(HOOK_VIEWPORTS);
    if forward.is_null() {
        return;
    }
    let admission = Admission::enter(context);
    if inside() || admission.is_none() || !TAP.plan_active.load(SeqCst) {
        unsafe { (platform.set_viewports.unwrap())(forward, context, count, viewports) };
        return;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    // Remembered unscaled: the game's intent, which goes back when an unreplaced target is bound.
    render.game_viewport_count = 0;
    if !viewports.is_null() {
        let kept = (count as usize).min(VIEWPORTS);
        render.game_viewports[..kept]
            .copy_from_slice(unsafe { slice::from_raw_parts(viewports, kept) });
        render.game_viewport_count = kept;
    }
    if !TAP.target_substituted.load(Relaxed) || render.game_viewport_count == 0 {
        unsafe { (platform.set_viewports.unwrap())(forward, context, count, viewports) };
        return;
    }
    apply_viewport_policy(platform, render, context);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_scissors(
    platform: &Platform,
    context: Handle,
    count: u32,
    rectangles: *const rsf_frame_tap_rect,
) {
    refresh(platform);
    let forward = original(HOOK_SCISSORS);
    if forward.is_null() {
        return;
    }
    let admission = Admission::enter(context);
    if inside() || admission.is_none() || !TAP.plan_active.load(SeqCst) {
        unsafe { (platform.set_scissors.unwrap())(forward, context, count, rectangles) };
        return;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    render.game_scissor_count = 0;
    if !rectangles.is_null() {
        let kept = (count as usize).min(VIEWPORTS);
        render.game_scissors[..kept]
            .copy_from_slice(unsafe { slice::from_raw_parts(rectangles, kept) });
        render.game_scissor_count = kept;
    }
    if !TAP.target_substituted.load(Relaxed) || render.game_scissor_count == 0 {
        unsafe { (platform.set_scissors.unwrap())(forward, context, count, rectangles) };
        return;
    }
    apply_viewport_policy(platform, render, context);
}

// The clear follows the target: a replaced target that is never cleared keeps old frames. A
// substitute carries its admission to the detour's `leave`, after the clear is forwarded.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_clear_target(
    platform: &Platform,
    context: Handle,
    view: Handle,
    admitted: *mut u32,
) -> Handle {
    let Some(admission) = Admission::enter(context) else {
        return view;
    };
    if inside() || view.is_null() || !TAP.plan_active.load(SeqCst) {
        return view;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    // The bound target is nearly always what is cleared, and the shadow has its texture.
    let bound = if view == render.target_view {
        target_texture(render)
    } else {
        null_mut()
    };
    let owned = if bound.is_null() {
        texture_behind(platform, view)
    } else {
        None
    };
    let texture = owned.as_ref().map_or(bound, ComRef::as_ptr);
    if texture.is_null() {
        return view;
    }
    let replacement = applicable(texture, View::Render);
    if replacement.is_null() {
        return view;
    }
    unsafe { admission.carry(admitted) };
    replacement
}

// One draw the tap looks at, from the divert decision to the reports. The guard is taken before
// the divert, because retargeting issues context calls that are not the game's. The game's call
// reads its original afresh, after anything that may have moved the runtime's table.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_draw(
    platform: &Platform,
    context: Handle,
    hook: u32,
    call: &rsf_frame_tap_draw_call,
) {
    refresh(platform);
    let hook = hook as usize;
    if original(hook).is_null() {
        return;
    }
    let forward = || unsafe { (platform.draw.unwrap())(original(hook), context, call) };
    let admission = Admission::enter(context);
    if inside() || admission.is_none() {
        forward();
        refresh(platform);
        return;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    // Draw and DrawIndexed are also offered to research ahead of the game's call.
    let indexed = call.kind == 1 || call.kind == 3;
    let before = call.kind < 2;
    let elements = call.count;
    sync_shadow(platform, render, context);
    if !render.geometry_valid {
        seed_geometry(platform, render, context);
    }
    let moved = try_divert(platform, render, context, indexed, elements);
    if !moved {
        begin_constant_override(platform, render, context, indexed, elements);
    }
    if before {
        consider_target_draw(platform, render, context, indexed, elements, true);
        // A before-capture Map can switch the runtime's submission vtable.
        refresh(platform);
    }
    forward();
    refresh(platform);
    if moved {
        end_divert(platform, render, context);
    }
    end_constant_override(platform, render, context);
    report_geometry(platform, render, context, call, null_mut(), 0);
    consider_bound_set(platform, render, context);
    consider_target_draw(platform, render, context, indexed, elements, false);
}

// Draws whose arguments live in a buffer or come from stream-out, after they are forwarded.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_indirect(
    platform: &Platform,
    context: Handle,
    kind: u32,
    arguments: Handle,
    offset: u32,
) {
    refresh(platform);
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    let _guard = Reentry::enter();
    let call = rsf_frame_tap_draw_call {
        kind,
        count: 0,
        start: 0,
        instances: 1,
        first_instance: 0,
        base_vertex: 0,
    };
    report_geometry(
        platform,
        unsafe { render() },
        context,
        &call,
        arguments,
        offset,
    );
}

unsafe fn read_or<T: Copy>(items: *const T, index: usize, empty: T) -> T {
    if items.is_null() {
        empty
    } else {
        unsafe { *items.add(index) }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_vertex_buffers(
    context: Handle,
    start: u32,
    count: u32,
    buffers: *const Handle,
    strides: *const u32,
    offsets: *const u32,
) {
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    let geometry = unsafe { &mut render().geometry };
    for index in 0..count as usize {
        let slot = start as usize + index;
        if slot >= VERTEX_BUFFERS {
            break;
        }
        unsafe {
            geometry.vertex_buffers[slot] = read_or(buffers, index, null_mut());
            geometry.strides[slot] = read_or(strides, index, 0);
            geometry.offsets[slot] = read_or(offsets, index, 0);
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_index_buffer(
    context: Handle,
    buffer: Handle,
    format: u32,
    offset: u32,
) {
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    let geometry = unsafe { &mut render().geometry };
    geometry.index_buffer = buffer;
    geometry.index_format = format;
    geometry.index_offset = offset;
}

// The pipeline objects a draw is identified by. Recorded and nothing more: what they mean is
// settled at creation and looked up by address.
#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_object(context: Handle, object: u32, value: Handle) {
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    let render = unsafe { render() };
    let field = match object {
        OBJECT_LAYOUT => &mut render.geometry.input_layout,
        OBJECT_VERTEX_SHADER => &mut render.geometry.vertex_shader,
        OBJECT_PIXEL_SHADER => &mut render.pixel_shader,
        OBJECT_BLEND => &mut render.blend_state,
        OBJECT_DEPTH_STENCIL => &mut render.depth_stencil_state,
        _ => return,
    };
    *field = value;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_topology(context: Handle, topology: u32) {
    let admission = Admission::enter(context);
    if !inside() && admission.is_some() {
        unsafe { render() }.geometry.topology = topology;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_vertex_constants(
    context: Handle,
    start: u32,
    count: u32,
    buffers: *const Handle,
) {
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    let constants = unsafe { &mut render().geometry.vertex_constants };
    for index in 0..count as usize {
        let slot = start as usize + index;
        if slot >= STAGE_CONSTANTS {
            break;
        }
        constants[slot] = unsafe { read_or(buffers, index, null_mut()) };
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_pixel_constants(
    platform: &Platform,
    context: Handle,
    start: u32,
    count: u32,
    buffers: *const Handle,
) {
    if count == 0 {
        return;
    }
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if inside() {
        return;
    }
    // The view buffer only goes to the pass callback, so without one nothing asks a buffer its
    // size. A slot rebound to what it held was looked at when first bound.
    let options = options();
    let wanted = if options.on_pass.is_some() {
        options.view_constant_bytes
    } else {
        0
    };
    let render = unsafe { render() };
    let mut view = null_mut();
    for index in 0..count as usize {
        let buffer = unsafe { read_or(buffers, index, null_mut()) };
        let slot = start as usize + index;
        if slot < STAGE_CONSTANTS {
            let unchanged = render.pixel_constants[slot] == buffer;
            render.pixel_constants[slot] = buffer;
            if unchanged {
                continue;
            }
        }
        if wanted == 0 || buffer.is_null() || !view.is_null() {
            continue;
        }
        // The most recent of the size: older ones describe a frame nobody asked about. Which of
        // them holds view data is the caller's question.
        let mut bind_flags = 0;
        if unsafe { (platform.buffer_width.unwrap())(buffer, &mut bind_flags) } == wanted {
            view = buffer;
        }
    }
    if !view.is_null() && view != TAP.view_constants_hint.load(Relaxed) {
        keep_view_constants(view);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_invalidate(context: Handle, restore: u32) {
    if restore != 0 {
        return;
    }
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    if !inside() {
        invalidate_geometry(unsafe { render() });
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_map(
    platform: &Platform,
    context: Handle,
    resource: Handle,
    subresource: u32,
    kind: u32,
    flags: u32,
    mapped: *mut MappedResource,
) -> i32 {
    refresh(platform);
    let admission = Admission::enter(context);
    let forward: Map = unsafe { std::mem::transmute(original(RSF_FRAME_TAP_HOOK_MAP as usize)) };
    let result = unsafe { forward(context, resource, subresource, kind, flags, mapped) };
    refresh(platform);
    if result < 0
        || admission.is_none()
        || mapped.is_null()
        || resource.is_null()
        || kind != MAP_WRITE_DISCARD
        || subresource != 0
        || inside()
    {
        return result;
    }
    let data = unsafe { (*mapped).data };
    if data.is_null() {
        return result;
    }
    let Some(watch) = *constant_watch() else {
        return result;
    };
    let bytes = watched_buffer(platform, unsafe { render() }, resource, watch.bytes);
    if bytes != 0 {
        let mut mappings = pending();
        let index = mappings
            .iter()
            .position(|entry| {
                entry.is_some_and(|entry| entry.context == context && entry.resource == resource)
            })
            .or_else(|| mappings.iter().position(Option::is_none));
        if let Some(index) = index {
            mappings[index] = Some(Pending {
                context,
                resource,
                data,
                bytes,
            });
        }
    }
    result
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_unmap(
    platform: &Platform,
    context: Handle,
    resource: Handle,
    subresource: u32,
) {
    refresh(platform);
    let admission = Admission::enter(context);
    let forward: Unmap =
        unsafe { std::mem::transmute(original(RSF_FRAME_TAP_HOOK_UNMAP as usize)) };
    if admission.is_some() && subresource == 0 {
        let entry = pending()
            .iter_mut()
            .find(|entry| {
                entry.is_some_and(|entry| entry.context == context && entry.resource == resource)
            })
            .and_then(Option::take);
        if let Some(entry) = entry
            && !inside()
        {
            let _guard = Reentry::enter();
            let watch = *constant_watch();
            if let Some(watch) = watch {
                unsafe { watch.notify(platform, resource, entry.data, entry.bytes) };
            }
        }
    }
    unsafe { forward(context, resource, subresource) };
    refresh(platform);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_frame_tap_dispatch(
    platform: &Platform,
    context: Handle,
    x: u32,
    y: u32,
    z: u32,
    arguments: Handle,
    offset: u32,
) {
    let compute: rsf_frame_tap_compute_fn = unsafe { load_fn(&TAP.research_compute) };
    if compute.is_none() || inside() {
        return;
    }
    // Admitted so uninstall does not return while the research callback is still running.
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    let _guard = Reentry::enter();
    unsafe {
        (platform.compute.unwrap())(
            compute,
            TAP.research_user.load(Relaxed),
            context,
            x,
            y,
            z,
            arguments,
            offset,
        )
    };
}

// The stand-in for a promoted resource whose substitution applies now.
fn promoted(platform: &Platform, resource: Handle) -> Option<ComRef> {
    let view = applicable(resource, View::Render);
    if view.is_null() {
        return None;
    }
    unsafe { ComRef::from_owned((platform.view_resource.unwrap())(view)) }
}

// A copy between promoted textures goes between their stand-ins, so a result copied from one
// promoted target into another arrives at output resolution. A redirected copy carries its
// admission to the detour's `leave`.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_copy(
    platform: &Platform,
    context: Handle,
    destination: *mut Handle,
    source: *mut Handle,
    admitted: *mut u32,
) {
    let Some(admission) = Admission::enter(context) else {
        return;
    };
    if inside() || !TAP.plan_active.load(SeqCst) {
        return;
    }
    let _guard = Reentry::enter();
    let promoted_destination = promoted(platform, unsafe { *destination });
    let promoted_source = promoted(platform, unsafe { *source });
    match (&promoted_destination, &promoted_source) {
        (Some(promoted_destination), Some(promoted_source)) => {
            unsafe {
                *destination = promoted_destination.as_ptr();
                *source = promoted_source.as_ptr();
                admission.carry(admitted);
            }
            bump(COPIES_REDIRECTED);
        }
        // Half a pair would copy between different sizes, which D3D11 drops.
        (Some(_), None) | (None, Some(_)) => bump(COPIES_MISMATCHED),
        (None, None) => {}
    }
}

// A shader's own constants on their way to the GPU. The watch sees them on a copy unless it only
// reads: the game's shadow is the game's, and the next upload must start from what it asked for.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn rsf_core_frame_tap_update(
    platform: &Platform,
    context: Handle,
    destination: Handle,
    subresource: u32,
    has_box: u32,
    data: *const c_void,
    row_pitch: u32,
    scratch: Handle,
) -> *const c_void {
    if inside()
        || destination.is_null()
        || data.is_null()
        || has_box != 0
        || subresource != 0
        || row_pitch == 0
        || row_pitch > 4096
    {
        return data;
    }
    // What is forwarded is the game's data or the detour's scratch, so nothing outlives this.
    let Some(_admission) = Admission::enter(context) else {
        return data;
    };
    let Some(watch) = *constant_watch() else {
        return data;
    };
    let bytes = watched_buffer(platform, unsafe { render() }, destination, watch.bytes);
    if bytes == 0 || row_pitch > bytes {
        return data;
    }
    let _guard = Reentry::enter();
    bump(UPDATES_WATCHED);
    if !matches!(watch.callback, ConstantCallback::Legacy(_, true)) {
        unsafe { watch.notify(platform, destination, data, row_pitch) };
        return data;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data.cast::<u8>(), scratch.cast::<u8>(), row_pitch as usize);
        watch.notify(platform, destination, scratch, row_pitch);
    }
    scratch.cast_const()
}

// The samplers to forward. With no bias this is the game's own array, so the hook stays a plain
// forward: the bias reads the bindings back from the context when it starts or ends. Biased clones
// carry their admission to the detour's `leave`, so uninstall cannot release one being bound.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_frame_tap_samplers(
    platform: &Platform,
    context: Handle,
    start: u32,
    count: u32,
    samplers: *const Handle,
    bound: *mut Handle,
    admitted: *mut u32,
) -> *const Handle {
    let (start, count) = (start as usize, count as usize);
    if !TAP.sampler_bias_active.load(Acquire)
        || inside()
        || samplers.is_null()
        || count == 0
        || start >= SAMPLERS
        || count > SAMPLERS - start
    {
        return samplers;
    }
    let Some(admission) = Admission::enter(context) else {
        return samplers;
    };
    unsafe { admission.carry(admitted) };
    let mut state = sampler_state();
    let incoming = unsafe { slice::from_raw_parts(samplers, count) };
    let mut originals = [null_mut(); SAMPLERS];
    for (original, &sampler) in originals.iter_mut().zip(incoming) {
        *original = state.original_of(sampler);
    }
    let bound_slots = unsafe { slice::from_raw_parts_mut(bound, count) };
    for (bound, &original) in bound_slots.iter_mut().zip(&originals) {
        *bound = state.biased(platform, context, original);
    }
    bound.cast_const()
}
