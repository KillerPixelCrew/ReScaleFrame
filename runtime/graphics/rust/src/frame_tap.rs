// SPDX-License-Identifier: MIT
// Shared D3D11 frame tap. The Rust detours and platform leaves use the native COM ABI.
mod detours;
mod platform;

use crate::com_ref::ComRef;
use crate::constant_buffer_read::{MAP_WRITE_DISCARD, Map, MappedResource, Unmap, slot};
use crate::sys::*;
use std::cell::{Cell, UnsafeCell};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::{transmute_copy, zeroed};
use std::ptr::{null, null_mut};
use std::slice;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release, SeqCst};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};

type Handle = *mut c_void;
struct Platform {
    patch:
        unsafe extern "C" fn(vtable: *mut *mut c_void, slot: u32, replacement: *mut c_void) -> i32,
    context1: unsafe extern "C" fn(context: *mut c_void) -> i32,
    texture_behind: unsafe extern "C" fn(view: *mut c_void) -> *mut c_void,
    texture_facts: unsafe extern "C" fn(texture: *mut c_void, facts: *mut rsf_texture_facts),
    target_view_format: unsafe extern "C" fn(view: *mut c_void) -> u32,
    buffer_width: unsafe extern "C" fn(resource: *mut c_void, bind_flags: *mut u32) -> u32,
    shader_views: unsafe extern "C" fn(context: *mut c_void, views: *mut *mut c_void),
    set_shader_views: unsafe extern "C" fn(
        original: *mut c_void,
        context: *mut c_void,
        start: u32,
        count: u32,
        views: *const *mut c_void,
    ),
    set_targets: unsafe extern "C" fn(
        original: *mut c_void,
        context: *mut c_void,
        count: u32,
        views: *const *mut c_void,
        depth: *mut c_void,
        uavs: *const rsf_frame_tap_uavs,
    ),
    viewport:
        unsafe extern "C" fn(context: *mut c_void, viewport: *mut rsf_frame_tap_viewport) -> u32,
    stage_constants: unsafe extern "C" fn(
        context: *mut c_void,
        geometry: *mut *mut c_void,
        hull: *mut *mut c_void,
        domain: *mut *mut c_void,
    ),
    seed_geometry: unsafe extern "C" fn(
        context: *mut c_void,
        geometry: *mut rsf_frame_tap_geometry,
        pixel_constants: *mut *mut c_void,
    ),
    draw: unsafe extern "C" fn(
        original: *mut c_void,
        context: *mut c_void,
        call: *const rsf_frame_tap_draw_call,
    ),
    samplers: unsafe extern "C" fn(context: *mut c_void, samplers: *mut *mut c_void),
    set_samplers: unsafe extern "C" fn(
        original: *mut c_void,
        context: *mut c_void,
        start: u32,
        count: u32,
        samplers: *const *mut c_void,
    ),
    clone_sampler:
        unsafe extern "C" fn(context: *mut c_void, sampler: *mut c_void, bias: f32) -> *mut c_void,
    report: unsafe extern "C" fn(
        fn_: rsf_frame_tap_target_fn,
        user: *mut c_void,
        draw: *const rsf_frame_tap_target_draw,
    ),
    geometry: unsafe extern "C" fn(
        fn_: rsf_frame_tap_geometry_fn,
        user: *mut c_void,
        geometry: *const rsf_frame_tap_geometry,
    ),
    compute: unsafe extern "C" fn(
        fn_: rsf_frame_tap_compute_fn,
        user: *mut c_void,
        context: *mut c_void,
        x: u32,
        y: u32,
        z: u32,
        arguments: *mut c_void,
        offset: u32,
    ),
}

const OK: i32 = 0;
const INVALID: i32 = -1;
const ABI_MISMATCH: i32 = -2;
const ALREADY_INSTALLED: i32 = -3;
const NOT_INSTALLED: i32 = -4;
const PATCH_FAILED: i32 = -5;

// D3D11_KEEP_RENDER_TARGETS_AND_DEPTH_STENCIL, D3D11_MAP_WRITE_DISCARD, D3D11_BIND_CONSTANT_BUFFER.
const KEEP_TARGETS: u32 = u32::MAX;
const BIND_CONSTANT_BUFFER: u32 = 4;
// Slot 13 is Draw. One comparison against its replacement says whether the runtime has rewritten
// the work-submission family since the last look.
const SLOT_DRAW: usize = 13;

const HOOKS: usize = RSF_FRAME_TAP_HOOK_COUNT as usize;
const HOOK_VIEWS: usize = RSF_FRAME_TAP_HOOK_VIEWS as usize;
const HOOK_SAMPLERS: usize = RSF_FRAME_TAP_HOOK_SAMPLERS as usize;
const HOOK_DRAW: usize = RSF_FRAME_TAP_HOOK_DRAW as usize;
const HOOK_TARGETS: usize = RSF_FRAME_TAP_HOOK_TARGETS as usize;
const HOOK_TARGETS_AND_UAVS: usize = RSF_FRAME_TAP_HOOK_TARGETS_AND_UAVS as usize;
const OBJECT_LAYOUT: u32 = RSF_FRAME_TAP_OBJECT_LAYOUT as u32;
const OBJECT_VERTEX_SHADER: u32 = RSF_FRAME_TAP_OBJECT_VERTEX_SHADER as u32;
const OBJECT_PIXEL_SHADER: u32 = RSF_FRAME_TAP_OBJECT_PIXEL_SHADER as u32;
const OBJECT_BLEND: u32 = RSF_FRAME_TAP_OBJECT_BLEND as u32;
const OBJECT_DEPTH_STENCIL: u32 = RSF_FRAME_TAP_OBJECT_DEPTH_STENCIL as u32;

// Every slot D3D11 allows a stage. Unreal's scene textures alone can reach past slot 16.
const VIEWS: usize = 128;
const SAMPLERS: usize = 16;
const STAGE_CONSTANTS: usize = 14;
const VERTEX_BUFFERS: usize = 32;
const MAX_INPUTS: usize = RSF_FRAME_TAP_MAX_INPUTS as usize;

const VERDICT_SLOTS: usize = 64;
const VERDICT_LIFETIME: u32 = 16384;
const PENDING_MAPS: usize = 16;

const ZERO_FACTS: rsf_texture_facts = unsafe { zeroed() };
const ZERO_GEOMETRY: rsf_frame_tap_geometry = unsafe { zeroed() };
const ZERO_INPUT: rsf_frame_tap_input = unsafe { zeroed() };
const ZERO_OPTIONS: rsf_frame_tap_options = unsafe { zeroed() };
const ZERO_VIEWPORT: rsf_frame_tap_viewport = unsafe { zeroed() };

thread_local! {
    // Callbacks bind resources of their own, which come straight back through these hooks. A depth
    // rather than a flag: guards nest inside an outer hook's guard, and a flag would be cleared by
    // the inner one while the outer hook still runs.
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

// Hooks on the observed context that read the render-thread state hold an admission, counted as
// the overlay cursor and observer Present hooks count their calls. Uninstall closes a gate, which
// keeps new hooks out, and drains the admitted ones before it changes anything, so no hook on
// another thread borrows a shadow or a biased sampler being retired.
//
// A control call made from a tap callback already holds an admission further up its own stack.
// It never waits: it takes the control lock only if it is free and proceeds only if no other
// thread is admitted, and is otherwise refused before any change. Its own admissions stay valid:
// its retirement waits for the outermost admission to end.
static ADMITTED: AtomicU32 = AtomicU32::new(0);
// Closed while a control entry drains and changes state. Nested calls of an admitted hook still
// pass, since the entry may be waiting for that very hook.
static GATE_CLOSED: AtomicBool = AtomicBool::new(false);
// The render state is retired by whoever ends the last admission after an uninstall. The lock
// keeps an install from seeding the shadow while it is being retired.
static RETIRE_PENDING: AtomicBool = AtomicBool::new(false);
static RETIRING: Mutex<()> = Mutex::new(());
// Serialises install and uninstall. Hooks never take it, and an admitted caller never
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
    unsafe { (platform.patch)(vtable, slot as u32, replacement) != 0 }
}

// Put back the originals of patched hooks, in the given order.
fn restore(platform: &Platform, vtable: *mut Handle, hooks: impl Iterator<Item = usize>) -> bool {
    let mut restored = true;
    for hook in hooks {
        if HOOK_TABLE.replacements[hook].load(Acquire).is_null() {
            continue;
        }
        let slot = HOOK_TABLE.slots[hook].load(Relaxed) as u32;
        if unsafe { (platform.patch)(vtable, slot, original(hook)) } != 0 {
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
static HOOK_COVERAGE: AtomicBool = AtomicBool::new(false);

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
    let mut complete = true;
    for (hook, replacement) in HOOK_TABLE.replacements.iter().enumerate() {
        let replacement = replacement.load(Relaxed);
        if replacement.is_null() {
            continue;
        }
        let slot = HOOK_TABLE.slots[hook].load(Relaxed);
        if unsafe { vtable.add(slot).read_volatile() } == replacement {
            continue;
        }
        complete &= patch(platform, vtable, hook, replacement);
    }
    HOOK_COVERAGE.store(complete, Release);
}

struct Tap {
    installed: AtomicBool,
    context: AtomicPtr<c_void>,
    // Written by install while not installed, read without the lock afterwards.
    options: UnsafeCell<rsf_frame_tap_options>,
    shadow_needed: AtomicBool,
    research_draw: AtomicPtr<c_void>,
    research_before: AtomicPtr<c_void>,
    research_compute: AtomicPtr<c_void>,
    research_user: AtomicPtr<c_void>,
    verdict_epoch: AtomicU32,
    sampler_bias_active: AtomicBool,
}

// Cross-thread state is atomic; the options follow the publication rule above.
unsafe impl Sync for Tap {}

static TAP: Tap = Tap {
    installed: AtomicBool::new(false),
    context: AtomicPtr::new(null_mut()),
    options: UnsafeCell::new(ZERO_OPTIONS),
    shadow_needed: AtomicBool::new(false),
    research_draw: AtomicPtr::new(null_mut()),
    research_before: AtomicPtr::new(null_mut()),
    research_compute: AtomicPtr::new(null_mut()),
    research_user: AtomicPtr::new(null_mut()),
    verdict_epoch: AtomicU32::new(0),
    sampler_bias_active: AtomicBool::new(false),
};

fn options() -> &'static rsf_frame_tap_options {
    unsafe { &*TAP.options.get() }
}
fn observed(context: Handle) -> bool {
    context == TAP.context.load(Relaxed)
}

// What the pixel stage has bound, as far as the tap has seen it. Unreal binds a slot per call, so
// the set is shadowed across calls and reported at the draw. The texture behind each view is held.
struct Slot {
    view: Handle,
    texture: Option<ComRef>,
    facts: rsf_texture_facts,
}

const EMPTY_SLOT: Slot = Slot {
    view: null_mut(),
    texture: None,
    facts: ZERO_FACTS,
};

// Render-thread state of the observed context. Reached once the reentry depth says a call is the
// game's, so a callback's nested hooks never alias it. Uninstall reaches it from its own thread,
// as the native owner did.
struct Render {
    slots: [Slot; VIEWS],
    occupied: [u64; VIEWS / 64],
    // What the render thread last acted on of `shadow_needed`.
    shadow_live: bool,
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
        let clone =
            unsafe { ComRef::from_owned((platform.clone_sampler)(context, original, bias)) };
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
    unsafe { ComRef::from_owned((platform.texture_behind)(view)) }
}

fn facts_of(platform: &Platform, texture: Handle) -> rsf_texture_facts {
    let mut facts = ZERO_FACTS;
    unsafe { (platform.texture_facts)(texture, &mut facts) };
    facts
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
    }
    render.target_texture = texture;
    // For a typeless target the view, not the texture, says whether output is encoded.
    render.target_view_format = unsafe { (platform.target_view_format)(view) };
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
        }
    }
}

// What the shadow knows of a draw. Inputs and constant buffers are the caller's to add.
fn common_facts(render: &Render, indexed: bool, elements: u32) -> rsf_frame_tap_target_draw {
    let mut facts: rsf_frame_tap_target_draw = unsafe { zeroed() };
    facts.struct_size = size_of::<rsf_frame_tap_target_draw>() as u32;
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

// Geometry, hull and domain constants, asked only on a draw that is offered.
fn stage_constants(platform: &Platform, context: Handle, facts: &mut rsf_frame_tap_target_draw) {
    unsafe {
        (platform.stage_constants)(
            context,
            facts.geometry_constants.as_mut_ptr(),
            facts.hull_constants.as_mut_ptr(),
            facts.domain_constants.as_mut_ptr(),
        )
    };
}

// Offer this draw to the research callbacks. `before` offers it ahead of the game's call.
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
    let target = target_texture(render);
    if target.is_null() {
        return;
    }
    let ordinal = render.draws_into_target;
    if !before {
        render.draws_into_target = ordinal.wrapping_add(1);
    }
    if research.is_none() || !observed(context) {
        return;
    }

    let mut inputs = [ZERO_INPUT; MAX_INPUTS];
    let mut report = common_facts(render, indexed, elements);
    report.context = context;
    report.draw_index = ordinal;
    collect_inputs(render, &mut report, &mut inputs);
    // The one fact the shadow cannot supply, asked only on a reported draw.
    let mut viewport = ZERO_VIEWPORT;
    if unsafe { (platform.viewport)(context, &mut viewport) } >= 1 {
        report.viewport_x = viewport.x;
        report.viewport_y = viewport.y;
        report.viewport_width = viewport.width as u32;
        report.viewport_height = viewport.height as u32;
    }
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
    unsafe { (platform.report)(research, TAP.research_user.load(Relaxed), &report) };
}

// Whether anything reads the shader resource shadow. Without a reader, keeping it is a resource
// query per binding for nothing.
fn refresh_shadow_need() {
    let needed = !TAP.research_draw.load(Relaxed).is_null()
        || !TAP.research_before.load(Relaxed).is_null();
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
    if !wanted {
        for index in occupied(render) {
            drop_slot(render, index);
        }
        return;
    }
    let mut views = [null_mut(); VIEWS];
    unsafe { (platform.shader_views)(context, views.as_mut_ptr()) };
    for (index, view) in views.into_iter().enumerate() {
        if let Some(view) = unsafe { ComRef::from_owned(view) } {
            shadow_view(platform, render, index, view.as_ptr(), None);
        }
    }
}

// Seed inherited IA/VS/PS constant state on the render thread, also after ClearState or a command
// list. The context owns the live bindings.
fn seed_geometry(platform: &Platform, render: &mut Render, context: Handle) {
    unsafe {
        (platform.seed_geometry)(
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
    unsafe { (platform.geometry)(options.on_geometry, options.on_geometry_user, geometry) };
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
    for index in occupied(render) {
        drop_slot(render, index);
    }
    forget_target(render);
}

// Everything the tap holds of the game's: the shadowed bindings and the render target. Run once
// per uninstall, when no hook holds an admission any longer.
fn retire_render() {
    let _retiring = retiring();
    if !RETIRE_PENDING.load(SeqCst) {
        return;
    }
    let render = unsafe { render() };
    pending().fill(None);
    invalidate_geometry(render);
    render.shadow_live = false;
    RETIRE_PENDING.store(false, SeqCst);
}

#[derive(Clone, Copy)]
struct ConstantWatch {
    callback: rsf_frame_tap_constant_observer_fn,
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

unsafe extern "C" {
    fn rsf_frame_tap_call_constant_observer(
        callback: rsf_frame_tap_constant_observer_fn,
        user: Handle,
        resource: Handle,
        data: *const c_void,
        bytes: u32,
    );
}

impl ConstantWatch {
    unsafe fn notify(self, resource: Handle, data: *const c_void, bytes: u32) {
        unsafe { rsf_frame_tap_call_constant_observer(self.callback, self.user, resource, data, bytes) }
    }
}

// Whether `resource` is a constant buffer the watch wants, and its width. Only a "no" is
// remembered: a width becomes a memory size the caller copies, and a recycled address must not be
// taken for it.
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
    let width = unsafe { (platform.buffer_width)(resource, &mut bind_flags) };
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

unsafe fn rsf_core_frame_tap_install(
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
    unsafe { *TAP.options.get() = *options };
    TAP.context.store(context, Relaxed);

    // Keep the context alive until all writes back to its table have succeeded.
    let vtable = unsafe { *context.cast::<*mut Handle>() };
    unsafe { *HOOK_TABLE.context.get() = Some(ComRef::retain(context)) };
    HOOK_TABLE.vtable.store(vtable, Relaxed);
    let hooks = unsafe { slice::from_raw_parts(hooks, HOOKS) };
    HOOK_TABLE
        .sentinel
        .store(hooks[HOOK_DRAW].replacement, Relaxed);
    let context1 = unsafe { (platform.context1)(context) } != 0;
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

    unsafe { render() }.shadow_live = false;

    TAP.verdict_epoch.fetch_add(1, Relaxed);
    HOOK_COVERAGE.store(true, Release);
    TAP.installed.store(true, SeqCst);
    refresh_shadow_need();
    drop(retiring);
    drop(lock);
    OK
}


fn rsf_core_frame_tap_uninstall(platform: &Platform) -> i32 {
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
    // No hook on another thread is admitted from here on: none is using the shadow or a biased
    // clone, and none starts until the gate reopens on a session that is gone.

    let Some(_writer) = drain(lock) else {
        return INVALID;
    };
    TAP.research_draw.store(null_mut(), Release);
    TAP.research_before.store(null_mut(), Release);
    TAP.research_compute.store(null_mut(), Release);
    *constant_watch() = None;
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    TAP.shadow_needed.store(false, Relaxed);
    // Off before the table goes back, so a hook in flight cannot re-apply it or be admitted. Each
    // slot gets whatever the runtime last had there.
    HOOK_COVERAGE.store(false, Release);
    TAP.installed.store(false, SeqCst);
    let restored = restore(platform, vtable, 0..HOOKS);
    RETIRE_PENDING.store(true, SeqCst);
    {
        let mut samplers = sampler_state();
        samplers.biased.clear();
        samplers.clones.clear();
        samplers.bias = 0.0;
        TAP.sampler_bias_active.store(false, Release);
    }
    TAP.context.store(null_mut(), Relaxed);

    // From inside a callback this thread's own hook is still on the stack and retires the shadow
    // when its admission ends.
    if !admitted_here() {
        retire_render();
    }
    if restored { OK } else { PATCH_FAILED }
}

fn rsf_core_frame_tap_refresh(platform: &Platform) -> i32 {
    if !TAP.installed.load(Acquire) {
        return NOT_INSTALLED;
    }
    refresh(platform);
    // Once a frame from the present hook, when resource address lookups stop being trusted.
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    OK
}

fn rsf_core_frame_tap_set_sampler_bias(platform: &Platform, bias: f32) -> i32 {
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
    unsafe { (platform.samplers)(context, current.as_mut_ptr()) };
    let current = current.map(|sampler| unsafe { ComRef::from_owned(sampler) });
    let originals = current
        .each_ref()
        .map(|sampler| state.original_of(sampler.as_ref().map_or(null_mut(), ComRef::as_ptr)));
    let bound = originals.map(|sampler| state.biased(platform, context, sampler));
    unsafe { (platform.set_samplers)(forward, context, 0, SAMPLERS as u32, bound.as_ptr()) };
    drop(current);
    TAP.sampler_bias_active.store(bias != 0.0, Release);
    OK
}

#[unsafe(no_mangle)]
extern "C" fn rsf_frame_tap_set_constant_observer(
    bytes: u32,
    callback: rsf_frame_tap_constant_observer_fn,
    user: Handle,
) -> i32 {
    *constant_watch() = callback.map(|callback| ConstantWatch {
        callback: Some(callback),
        user,
        bytes,
    });
    TAP.verdict_epoch.fetch_add(1, Relaxed);
    OK
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

fn rsf_core_frame_tap_enter(platform: &Platform, hook: i32) -> Handle {
    refresh(platform);
    original(hook as usize)
}

fn rsf_core_frame_tap_leave(platform: &Platform, admitted: u32) {
    refresh(platform);
    if admitted != 0 {
        // Ends the admission a deciding entry carried across the detour's forward.
        drop(Admission);
    }
}

unsafe fn rsf_core_frame_tap_views(
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
    let admission = Admission::enter(context);
    // Forwarded before the shadow, so it is in place before a callback binds something else.
    unsafe { (platform.set_shader_views)(forward, context, start, count, views) };
    if inside() || admission.is_none() {
        return;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    sync_shadow(platform, render, context);
    // Only a slot whose view changed pays for a resource query.
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
        if render.slots[slot].view != view {
            shadow_view(platform, render, slot, view, None);
        }
    }
}

unsafe fn rsf_core_frame_tap_targets(
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
    unsafe { (platform.set_targets)(forward, context, count, views, depth, uavs) };
    // KEEP leaves the render targets alone, so the shadow leaves them too.
    let keeps = !uavs.is_null() && count == KEEP_TARGETS;
    if inside() || admission.is_none() || keeps {
        return;
    }
    let _guard = Reentry::enter();
    let render = unsafe { render() };
    render.depth_bound = !depth.is_null();
    render.geometry_depth = depth;
    render.target_count = count;
    let first = if views.is_null() || count == 0 {
        null_mut()
    } else {
        unsafe { *views }
    };
    shadow_target(platform, render, first, None);
    unbind_target_reads(render);
}

// One draw the tap looks at: offered to research before and after it is forwarded, and its
// geometry reported. The game's call reads its original afresh, after anything that may have
// moved the runtime's table.
fn rsf_core_frame_tap_draw(
    platform: &Platform,
    context: Handle,
    hook: i32,
    call: &rsf_frame_tap_draw_call,
) {
    refresh(platform);
    let hook = hook as usize;
    if original(hook).is_null() {
        return;
    }
    let forward = || unsafe { (platform.draw)(original(hook), context, call) };
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
    if before {
        consider_target_draw(platform, render, context, indexed, elements, true);
        // A before-capture Map can switch the runtime's submission vtable.
        refresh(platform);
    }
    forward();
    refresh(platform);
    report_geometry(platform, render, context, call, null_mut(), 0);
    consider_target_draw(platform, render, context, indexed, elements, false);
}

// Draws whose arguments live in a buffer or come from stream-out, after they are forwarded.
fn rsf_core_frame_tap_indirect(
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

unsafe fn rsf_core_frame_tap_vertex_buffers(
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

fn rsf_core_frame_tap_index_buffer(context: Handle, buffer: Handle, format: u32, offset: u32) {
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
fn rsf_core_frame_tap_object(context: Handle, object: u32, value: Handle) {
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

fn rsf_core_frame_tap_topology(context: Handle, topology: u32) {
    let admission = Admission::enter(context);
    if !inside() && admission.is_some() {
        unsafe { render() }.geometry.topology = topology;
    }
}

unsafe fn rsf_core_frame_tap_vertex_constants(
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

unsafe fn rsf_core_frame_tap_pixel_constants(
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
    let constants = unsafe { &mut render().pixel_constants };
    for index in 0..count as usize {
        let slot = start as usize + index;
        if slot >= STAGE_CONSTANTS {
            break;
        }
        constants[slot] = unsafe { read_or(buffers, index, null_mut()) };
    }
}

fn rsf_core_frame_tap_invalidate(context: Handle, restore: u32) {
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

unsafe fn rsf_core_frame_tap_map(
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

unsafe fn rsf_core_frame_tap_unmap(
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
                unsafe { watch.notify(resource, entry.data, entry.bytes) };
            }
        }
    }
    unsafe { forward(context, resource, subresource) };
    refresh(platform);
}

fn rsf_core_frame_tap_dispatch(
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
        (platform.compute)(
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

// A whole-buffer UpdateSubresource of a shader's own constants on its way to the GPU, offered to
// the observer before it is forwarded.
unsafe fn rsf_core_frame_tap_update(
    platform: &Platform,
    context: Handle,
    destination: Handle,
    subresource: u32,
    whole: bool,
    data: *const c_void,
    row_pitch: u32,
) {
    if inside()
        || destination.is_null()
        || data.is_null()
        || !whole
        || subresource != 0
        || row_pitch == 0
        || row_pitch > 4096
    {
        return;
    }
    let Some(_admission) = Admission::enter(context) else {
        return;
    };
    let Some(watch) = *constant_watch() else {
        return;
    };
    let bytes = watched_buffer(platform, unsafe { render() }, destination, watch.bytes);
    if bytes == 0 || row_pitch > bytes {
        return;
    }
    let _guard = Reentry::enter();
    unsafe { watch.notify(destination, data, row_pitch) };
}

// The samplers to forward. With no bias this is the game's own array, so the hook stays a plain
// forward: the bias reads the bindings back from the context when it starts or ends. Biased clones
// carry their admission to the detour's `leave`, so uninstall cannot release one being bound.
unsafe fn rsf_core_frame_tap_samplers(
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
