// SPDX-License-Identifier: MIT
//! The AC7 native renderer, formerly native_renderer.cpp: the renderer object, the detours on the
//! exact-build engine sites (sites.rs) and their engine relays, the queued pass leases and the
//! TrueSky relay handlers. renderer.rs owns the prepare/start/quiesce/stop transaction and reaches
//! this module through the unchanged `rsf_ac7_renderer_leaves` table (ac7_sites.h).
//!
//! Every detour keeps the former signature, guard kind, engine offsets, refusals and log text.
//! Engine memory is read through the SEH-guarded C leaf (ac7_memory.c) exactly where the former
//! code used it; the former unguarded engine writes stay unguarded. The one C++ exception the
//! former code contained, from the engine's pixel shader factory, is contained by the C leaf
//! native_renderer_platform.c. Every other engine call ran uncontained before and still does.
//! Research and the per-hook comparison are in docs/research/rust-ac7-native-renderer.md.
use crate::render_scope::rsf_ac7_render_scope_close;
use crate::render_scope::rsf_ac7_render_scope_enqueue_leased;
use crate::render_scope::rsf_ac7_render_scope_open;
use crate::render_scope::rsf_ac7_render_scope_open_leased;
use crate::render_scope::rsf_ac7_render_scopes_destroy;
use crate::render_scope::rsf_ac7_render_scopes_idle;
use crate::render_scope::rsf_ac7_render_scopes_quiesce;
use crate::scope_facts::rsf_ac7_scope_primary_facts;
use crate::scope_facts::rsf_ac7_scope_primary_renderer;
use crate::scope_facts::rsf_ac7_scope_primary_view;
use crate::sites::rsf_ac7_hook_role;
use crate::sites::rsf_ac7_hook_rva;
use crate::sys::rsf_ac7_cloud_depth;
use crate::sys::{
    RSF_AC7_GUARD_ENTRY, RSF_AC7_GUARD_OUTER, RSF_AC7_HOOK_SITE_COUNT, rsf_ac7_code_patches,
    rsf_ac7_native_calls, rsf_ac7_truesky_patches,
};
use crate::sys::{rsf_ac7_copy_memory, rsf_ac7_read_memory};
use crate::sys::{
    rsf_ac7_native_renderer, rsf_ac7_native_renderer_options, rsf_ac7_render_scope,
    rsf_ac7_render_scope_fn,
};
use crate::sys::{rsf_ac7_scope_lease, rsf_ac7_scope_read};
use rsf_game_sys::ffi_util::try_box;
use rsf_game_sys::rsf_game_render_config;
use std::alloc::{Layout, alloc_zeroed};
use std::cell::UnsafeCell;
use std::ffi::{CStr, CString, c_void};
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use windows::Win32::Graphics::Direct3D11::{ID3D11ComputeShader, ID3D11Device};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

mod evidence;
mod frame;
mod graph;
mod leaves;
pub(crate) use leaves::renderer_leaves;
mod particles;
mod shaders;
mod sky;
mod slate;
mod translucency;
mod views;
mod widgets;

#[cfg(feature = "warp-fixture")]
#[path = "../../../tests/ac7_native_graph.rs"]
mod fixture;

/// Index of each hooked function in `SITES` and in the Rust site table (sites.rs), in table order.
#[allow(dead_code)]
pub(crate) mod hook {
    pub(crate) const PROCESS_TONEMAP: u32 = 0;
    pub(crate) const PROCESS_AA: u32 = 1;
    pub(crate) const PROCESS_MATERIAL: u32 = 2;
    pub(crate) const PROCESS_HUD: u32 = 3;
    pub(crate) const PROCESS_COMPOSITE: u32 = 4;
    pub(crate) const PROCESS_OUTPUT: u32 = 5;
    pub(crate) const DESCRIPTOR_TONEMAP: u32 = 6;
    pub(crate) const DESCRIPTOR_AA: u32 = 7;
    pub(crate) const DESCRIPTOR_MATERIAL: u32 = 8;
    pub(crate) const DESCRIPTOR_HUD: u32 = 9;
    pub(crate) const DESCRIPTOR_COMPOSITE: u32 = 10;
    pub(crate) const DESCRIPTOR_OUTPUT: u32 = 11;
    pub(crate) const POSTPROCESS: u32 = 12;
    pub(crate) const CONTEXT: u32 = 13;
    pub(crate) const VISIBILITY: u32 = 14;
    pub(crate) const CONSTRUCT: u32 = 15;
    pub(crate) const WIDGET_QUEUE: u32 = 16;
    pub(crate) const WIDGET_TARGETS: u32 = 17;
    pub(crate) const WIDGET_DRAW: u32 = 18;
    pub(crate) const ENGINE_TICK: u32 = 19;
    pub(crate) const RENDERER_RETIRE: u32 = 20;
    pub(crate) const POLL_INPUT: u32 = 21;
    pub(crate) const GAME_INSTANCE_INIT: u32 = 22;
    pub(crate) const TARGET_INIT: u32 = 23;
    pub(crate) const SIMULATION_A: u32 = 24;
    pub(crate) const SIMULATION_B: u32 = 25;
    pub(crate) const REDRAW: u32 = 26;
    pub(crate) const VIEWPORT_DRAW: u32 = 27;
    pub(crate) const SLATE_PRIVATE: u32 = 28;
    pub(crate) const SLATE_ALLOCATE: u32 = 29;
    pub(crate) const SLATE_TASK: u32 = 30;
    pub(crate) const SLATE_WINDOW: u32 = 31;
    pub(crate) const SLATE_TEXTURE: u32 = 32;
    pub(crate) const TRANSLUCENCY_SIZE: u32 = 33;
    pub(crate) const UNMODIFIED_BEGIN: u32 = 34;
    pub(crate) const UNMODIFIED_RESOLVE: u32 = 35;
    pub(crate) const TRANSLUCENCY_RENDER: u32 = 36;
    pub(crate) const PIXEL_VIEW_UNIFORM: u32 = 37;
    pub(crate) const RHI_PIXEL_UNIFORM: u32 = 38;
    pub(crate) const RHI_PIXEL_TABLES: u32 = 39;
    pub(crate) const PIXEL_ENQUEUE_A: u32 = 40;
    pub(crate) const PIXEL_ENQUEUE_B: u32 = 41;
    pub(crate) const PIXEL_ENQUEUE_C: u32 = 42;
    pub(crate) const PIXEL_ENQUEUE_D: u32 = 43;
    pub(crate) const SKY_PROJECTION: u32 = 44;
    pub(crate) const SKY_RENDER: u32 = 45;
    pub(crate) const TEMPORAL_SAMPLE_INDEX: u32 = 46;
    pub(crate) const SCENE_COLOUR_FORMAT: u32 = 47;
    pub(crate) const RENDER_FAMILY: u32 = 48;
    pub(crate) const FRAME_TASK_CONSTRUCT: u32 = 49;
    pub(crate) const FRAME_TASK_EXECUTE: u32 = 50;
    pub(crate) const RHI_FRAME_BEGIN: u32 = 51;
    pub(crate) const RHI_FRAME_END: u32 = 52;
    pub(crate) const FRAME_SYNC: u32 = 53;
    pub(crate) const PUMP_MESSAGES: u32 = 54;
    pub(crate) const ENGINE_PACING: u32 = 55;
    pub(crate) const CREATE_PIXEL_SHADER: u32 = 56;
    pub(crate) const BASE_PASS: u32 = 57;
    pub(crate) const TRANSLUCENCY_MESH: u32 = 58;
    pub(crate) const MESH_PARTICLE_PARAMETERS: u32 = 59;
    pub(crate) const BEAM_NO_NOISE: u32 = 60;
    pub(crate) const BEAM_NOISE: u32 = 61;
    pub(crate) const BEAM_INTERPOLATED_NOISE: u32 = 62;
    pub(crate) const RIBBON_FILL: u32 = 63;
    pub(crate) const ANIM_TRAIL_FILL: u32 = 64;
    pub(crate) const COUNT: u32 = 65;
}

const HOOKS: usize = hook::COUNT as usize;
const _: () = assert!(hook::COUNT == RSF_AC7_HOOK_SITE_COUNT);
const _: () = assert!(
    hook::RHI_PIXEL_UNIFORM == crate::sys::RSF_AC7_EVIDENCE_FIRST
        && hook::PIXEL_ENQUEUE_D == crate::sys::RSF_AC7_EVIDENCE_LAST
);
/// The pass hooks come first: one process and one descriptor hook per reviewed graph pass.
const PROCESS_HOOKS: u32 = crate::sys::RSF_AC7_PROCESS_HOOKS;
const _: () = assert!(hook::PROCESS_OUTPUT + 1 == PROCESS_HOOKS);

/// MinHook state of one hooked entry site. The site's RVA, expected bytes and pass role are
/// owned by sites.rs.
struct Site {
    target: AtomicPtr<c_void>,
    original: AtomicPtr<c_void>,
    detour: AtomicPtr<c_void>,
}

static SITES: [Site; HOOKS] = [const {
    Site {
        target: AtomicPtr::new(null_mut()),
        original: AtomicPtr::new(null_mut()),
        detour: AtomicPtr::new(null_mut()),
    }
}; HOOKS];
static INSTALLED: AtomicPtr<Renderer> = AtomicPtr::new(null_mut());
static NATIVE: AtomicPtr<rsf_ac7_native_calls> = AtomicPtr::new(null_mut());

/// Binds the native call table (ac7_sites.h) that plugin.c and the native graph fixture lend, so
/// this archive holds no link-time reference to MinHook, the Windows input facts, the render scope
/// create call or the shader factory leaf. Every other rsf-ac7 consumer links without them.
///
/// # Safety
/// calls is null or a complete table that outlives the module.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_native_renderer_bind(calls: *const rsf_ac7_native_calls) {
    NATIVE.store(calls.cast_mut(), Ordering::Release);
}

/// The bound native call table, or None before plugin.c bound it.
fn native() -> Option<&'static rsf_ac7_native_calls> {
    unsafe { NATIVE.load(Ordering::Acquire).as_ref() }
}

/// The trampoline of `hook` as the detour's own function type.
///
/// # Safety
/// `F` is the function pointer type of the hooked engine function, and the hook is created, so
/// MinHook has written its trampoline. Every detour only runs once that holds.
unsafe fn original<F: Copy>(hook: u32) -> F {
    let address = SITES[hook as usize].original.load(Ordering::Relaxed);
    unsafe { function(address) }
}

/// # Safety
/// `address` is a nonnull function of type `F`.
unsafe fn function<F: Copy>(address: *mut c_void) -> F {
    const { assert!(size_of::<F>() == size_of::<*mut c_void>()) };
    unsafe { std::mem::transmute_copy(&address) }
}

fn site_target(hook: u32) -> *mut c_void {
    SITES[hook as usize].target.load(Ordering::Relaxed)
}

/// The published renderer every hook body reads. The renderer stays alive while hook bodies
/// can run: stop retires it only after the guards drain, and a retained one stays pinned.
fn installed() -> Option<&'static Renderer> {
    unsafe { INSTALLED.load(Ordering::Acquire).as_ref() }
}

/// Hook bodies count themselves in and out (guards.rs), so stop can tell when no thread is inside
/// this module.
struct Guard(u32);

impl Guard {
    fn outer() -> Self {
        crate::guards::rsf_ac7_guard_enter(RSF_AC7_GUARD_OUTER);
        Self(RSF_AC7_GUARD_OUTER)
    }

    fn entry() -> Self {
        crate::guards::rsf_ac7_guard_enter(RSF_AC7_GUARD_ENTRY);
        Self(RSF_AC7_GUARD_ENTRY)
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        crate::guards::rsf_ac7_guard_leave(self.0);
    }
}

#[cfg(feature = "warp-fixture")]
static TEST_MODULE: AtomicPtr<u8> = AtomicPtr::new(null_mut());
#[cfg(feature = "warp-fixture")]
static TEST_TRUESKY_MODULE: AtomicPtr<u8> = AtomicPtr::new(null_mut());
/// An `unsafe extern "C" fn(u32) -> *mut c_void` resolving engine helpers in the graph fixture.
#[cfg(feature = "warp-fixture")]
static TEST_ENGINE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// The game module. Only the graph fixture substitutes another image.
fn game_module() -> *mut u8 {
    #[cfg(feature = "warp-fixture")]
    {
        let module = TEST_MODULE.load(Ordering::Relaxed);
        if !module.is_null() {
            return module;
        }
    }
    static MODULE: AtomicPtr<u8> = AtomicPtr::new(null_mut());
    let mut module = MODULE.load(Ordering::Relaxed);
    if module.is_null() {
        module = unsafe { GetModuleHandleW(None) }.map_or(null_mut(), |module| module.0.cast());
        MODULE.store(module, Ordering::Relaxed);
    }
    module
}

/// The engine code or data at `rva`, which the graph fixture may resolve to its own helper.
fn engine_address(rva: u32) -> *mut c_void {
    #[cfg(feature = "warp-fixture")]
    {
        let resolve = TEST_ENGINE.load(Ordering::Relaxed);
        if !resolve.is_null() {
            let resolve: unsafe extern "C" fn(u32) -> *mut c_void = unsafe { function(resolve) };
            return unsafe { resolve(rva) };
        }
    }
    game_module().wrapping_add(rva as usize).cast()
}

/// The engine function at `rva` as `F`.
///
/// # Safety
/// `F` is the exact-build function type at `rva` (sites.rs admits the build first).
unsafe fn engine<F: Copy>(rva: u32) -> F {
    unsafe { function(engine_address(rva)) }
}

/// Copies live engine bytes under the guarded read. Unreadable memory returns false.
fn copy(output: *mut c_void, input: *const c_void, size: usize) -> bool {
    unsafe { rsf_ac7_copy_memory(output, input, size) }
}

/// Reads a `T` at `base + offset` under the guarded read; false leaves `output` as the read
/// left it, as the former template did.
fn read<T: Copy>(base: u64, offset: usize, output: &mut T) -> bool {
    unsafe {
        rsf_ac7_read_memory(
            (output as *mut T).cast(),
            base as usize,
            offset,
            size_of::<T>(),
        )
    }
}

fn at(base: u64) -> *mut c_void {
    base as usize as *mut c_void
}

fn key(pointer: *const c_void) -> u64 {
    pointer as usize as u64
}

/// The guarded read as the scope facts' reader type.
const SCOPE_READ: rsf_ac7_scope_read = Some(rsf_ac7_copy_memory);

/// True when `count` bytes of live code at `address` equal `expected`. Unreadable memory differs.
fn bytes_match(address: *const c_void, expected: *const c_void, count: usize) -> bool {
    let mut actual = [0u8; 16];
    count <= actual.len()
        && copy(actual.as_mut_ptr().cast(), address, count)
        && actual[..count]
            == unsafe { std::slice::from_raw_parts(expected.cast::<u8>(), count) }[..]
}

/// The engine singleton (GEngine), or 0 when it cannot be read.
fn game_engine_object() -> u64 {
    let mut object = 0u64;
    copy(
        (&mut object as *mut u64).cast(),
        engine_address(0x3cb_bc28),
        8,
    );
    object
}

/// Virtual slot `slot` (a byte offset into the vtable) of the engine object at `node`, or 0.
fn method(node: u64, slot: u32) -> u64 {
    let (mut table, mut entry) = (0u64, 0u64);
    if !read(node, 0, &mut table) || !read(table, slot as usize, &mut entry) {
        return 0;
    }
    entry
}

/// Calls the engine reference-count slot at `offset` (0x28 AddRef, 0x30 Release) of a pool.
fn pool_ref(pool: u64, offset: u32) {
    let entry = method(pool, offset);
    if entry != 0 {
        let call: unsafe extern "C" fn(*mut c_void) -> u32 = unsafe { function(at(entry)) };
        unsafe { call(at(pool)) };
    }
}

/// The native RHI resource of an engine texture (its GetNativeResource slot), or null.
fn native_texture(texture: u64) -> *mut c_void {
    let entry = method(texture, 0x30);
    if entry == 0 {
        return null_mut();
    }
    let call: unsafe extern "C" fn(*mut c_void) -> *mut c_void = unsafe { function(at(entry)) };
    unsafe { call(at(texture)) }
}

fn pool_texture(pool: u64, target: bool) -> *mut c_void {
    let mut texture = 0u64;
    if !read(pool, if target { 8 } else { 16 }, &mut texture) || texture == 0 {
        read(pool, 8, &mut texture);
    }
    native_texture(texture)
}

/// Adds one engine reference to the counted object at `pointer` (its count is at +8).
fn engine_add_ref(pointer: u64) {
    let count = (pointer as usize).wrapping_add(8) as *const std::sync::atomic::AtomicI32;
    unsafe { (*count).fetch_add(1, Ordering::SeqCst) };
}

/// The engine uniform move (RVA 0xde5cf0): releases `destination`'s reference and moves `source`
/// into it.
type MoveUniform = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;

fn move_uniform(destination: *mut c_void, source: *mut c_void) {
    unsafe { engine::<MoveUniform>(0xde_5cf0)(destination, source) };
}

/// Releases one engine uniform reference held in `uniform`.
fn release_uniform(uniform: &mut u64) {
    let mut empty = 0u64;
    move_uniform(
        (uniform as *mut u64).cast(),
        (&mut empty as *mut u64).cast(),
    );
}

/// A render scope with every field zero except its size.
fn scope() -> rsf_ac7_render_scope {
    let mut scope: rsf_ac7_render_scope = unsafe { zeroed() };
    scope.struct_size = size_of::<rsf_ac7_render_scope>() as u32;
    scope
}

fn render_config() -> rsf_game_render_config {
    let mut config: rsf_game_render_config = unsafe { zeroed() };
    config.struct_size = size_of::<rsf_game_render_config>() as u32;
    config
}

struct RendererIdentity {
    renderer: u64,
    source: u64,
    submission: u64,
    viewport: u64,
    after_simulation: bool,
    screen: u32,
    reset: u32,
}

#[derive(Clone, Copy)]
struct WindowSource {
    task: u64,
    source: u64,
    viewport: u64,
    window: u64,
    renderer: u64,
    info: u64,
    elements: u64,
    after_simulation: bool,
}

impl WindowSource {
    const EMPTY: Self = Self {
        task: 0,
        source: 0,
        viewport: 0,
        window: 0,
        renderer: 0,
        info: 0,
        elements: 0,
        after_simulation: false,
    };
}

struct FrameTask {
    task: u64,
    source: u64,
}

struct FinalSurfaceSource {
    source: u64,
    viewport: u64,
    surface: u64,
    ambiguous: bool,
}

struct UniformOwner {
    renderer: u64,
    view: u64,
    uniform: u64,
}

const FINAL_SURFACES: usize = 64;

/// Everything the former identities_guard protected. Every field is plain data, so the zeroed
/// allocation is the former value-initialized state except `next_submission`.
struct Tables {
    identities: [RendererIdentity; 512],
    next_submission: u64,
    previous_primary_screen: u32,
    previous_screen_why: u32,
    screen_lines: u32,
    previous_view_target: u64,
    window_tasks: [WindowSource; 512],
    frame_tasks: [FrameTask; 128],
    final_surfaces: [FinalSurfaceSource; FINAL_SURFACES],
    uniform_owners: [UniformOwner; 4096],
}

impl Tables {
    fn new() -> Option<Box<Self>> {
        let layout = Layout::new::<Self>();
        let raw = unsafe { alloc_zeroed(layout) }.cast::<Self>();
        if raw.is_null() {
            return None;
        }
        // Every field is an integer, bool or array of them, for which zero is valid.
        let mut tables = unsafe { Box::from_raw(raw) };
        tables.next_submission = 1;
        Some(tables)
    }
}

#[derive(Clone, Copy, Default)]
struct PixelUniformBinding {
    sequence: u64,
    context: u64,
    shader: u64,
    uniform: u64,
    command: u64,
    slot: u32,
}

struct PixelBindings {
    ring: [PixelUniformBinding; 128],
    sequence: u64,
}

/// The one-to-one depth shader of the TrueSky depth relay, changed only under `cloud_lock`.
struct Cloud {
    shader: Option<ID3D11ComputeShader>,
    device: Option<ID3D11Device>,
    shader_refused: bool,
}

/// The renderer object behind `rsf_ac7_native_renderer`.
pub(crate) struct Renderer {
    options: rsf_ac7_native_renderer_options,
    scopes: AtomicPtr<c_void>,
    active: AtomicBool,
    quiescing: AtomicBool,
    cloud_lock: Mutex<()>,
    /// TrueSky plugin patch and relay ownership, changed only by truesky.rs under `cloud_lock`.
    truesky: UnsafeCell<rsf_ac7_truesky_patches>,
    cloud: UnsafeCell<Cloud>,
    cloud_depth_ready: AtomicBool,
    cloud_depth_dispatches: AtomicU32,
    cloud_depth_binding_refusals: AtomicU32,
    /// Cloud motion depth resources, used by the composite_tile relay on the render thread and
    /// released by the TrueSky restore under `cloud_lock`.
    cloud_motion: UnsafeCell<rsf_ac7_cloud_depth>,
    cloud_motion_reports: AtomicU32,
    /// Scene-precision and temporal velocity patch ownership, changed only by patches.rs.
    patches: UnsafeCell<rsf_ac7_code_patches>,
    render_thread: AtomicU32,
    retired: AtomicPtr<PassLease>,
    tables: Mutex<Box<Tables>>,
    live_renderers: AtomicU32,
    next_source: AtomicU64,
    live_window_tasks: AtomicU32,
    live_frame_tasks: AtomicU32,
    input_hook: AtomicPtr<c_void>,
    contact_reports: AtomicU32,
    /// Surface of each final surface slot, readable without the lock. A Slate texture bind that
    /// does not match it cannot be the final scene surface, so most binds never take the lock.
    final_surface_hint: [AtomicU64; FINAL_SURFACES],
    pending_uniforms: AtomicU32,
    pending_particle_bindings: AtomicU32,
    pixel_bindings: Mutex<PixelBindings>,
    missing_pixel_reports: AtomicU32,
}

// The cells are changed only under `cloud_lock`, by the GameSDK transition that owns the patches,
// or by the render thread that owns the cloud motion resources, as in the former object.
unsafe impl Sync for Renderer {}
unsafe impl Send for Renderer {}

impl Renderer {
    fn create(options: &rsf_ac7_native_renderer_options) -> Option<Box<Self>> {
        let tables = Tables::new()?;
        try_box(Self {
            options: *options,
            scopes: AtomicPtr::new(null_mut()),
            active: AtomicBool::new(false),
            quiescing: AtomicBool::new(false),
            cloud_lock: Mutex::new(()),
            truesky: UnsafeCell::new(unsafe { zeroed() }),
            cloud: UnsafeCell::new(Cloud {
                shader: None,
                device: None,
                shader_refused: false,
            }),
            cloud_depth_ready: AtomicBool::new(false),
            cloud_depth_dispatches: AtomicU32::new(0),
            cloud_depth_binding_refusals: AtomicU32::new(0),
            cloud_motion: UnsafeCell::new(unsafe { zeroed() }),
            cloud_motion_reports: AtomicU32::new(0),
            patches: UnsafeCell::new(unsafe { zeroed() }),
            render_thread: AtomicU32::new(0),
            retired: AtomicPtr::new(null_mut()),
            tables: Mutex::new(tables),
            live_renderers: AtomicU32::new(0),
            next_source: AtomicU64::new(1),
            live_window_tasks: AtomicU32::new(0),
            live_frame_tasks: AtomicU32::new(0),
            input_hook: AtomicPtr::new(null_mut()),
            contact_reports: AtomicU32::new(0),
            final_surface_hint: [const { AtomicU64::new(0) }; FINAL_SURFACES],
            pending_uniforms: AtomicU32::new(0),
            pending_particle_bindings: AtomicU32::new(0),
            pixel_bindings: Mutex::new(PixelBindings {
                ring: [PixelUniformBinding::default(); 128],
                sequence: 0,
            }),
            missing_pixel_reports: AtomicU32::new(0),
        })
    }

    fn handle(&self) -> *mut rsf_ac7_native_renderer {
        (self as *const Self).cast_mut().cast()
    }

    /// # Safety
    /// `renderer` came from `handle` of a live renderer.
    unsafe fn from_handle<'a>(renderer: *mut rsf_ac7_native_renderer) -> &'a Self {
        unsafe { &*renderer.cast::<Self>() }
    }

    fn active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    fn deactivate(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    fn quiescing(&self) -> bool {
        self.quiescing.load(Ordering::SeqCst)
    }

    fn scopes(&self) -> *mut crate::render_scope::Scopes {
        self.scopes.load(Ordering::Relaxed).cast()
    }

    fn tables(&self) -> MutexGuard<'_, Box<Tables>> {
        self.tables.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The lock every change of `truesky` and `cloud` holds.
    fn cloud_guard(&self) -> MutexGuard<'_, ()> {
        self.cloud_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn log(&self, message: &CStr) {
        if let Some(log) = self.options.log {
            unsafe { log(self.options.user, message.as_ptr()) };
        }
    }

    fn log_text(&self, message: &str) {
        if let Ok(text) = CString::new(message) {
            self.log(&text);
        }
    }

    /// Logs at most `capacity - 1` bytes, as the former fixed snprintf buffer of `capacity`.
    fn log_bounded(&self, message: &str, capacity: usize) {
        let end = message.len().min(capacity.saturating_sub(1));
        self.log_text(message.get(..end).unwrap_or(message));
    }

    /// The host's current render configuration, or None when it has none.
    fn config(&self) -> Option<rsf_game_render_config> {
        let mut config = render_config();
        let callback = self.options.render_config?;
        (unsafe { callback(self.options.user, &mut config) } != 0).then_some(config)
    }

    fn on_stage(&self) -> rsf_ac7_render_scope_fn {
        self.options.on_stage
    }
}

fn primary_view(view: u64, config: &rsf_game_render_config) -> bool {
    rsf_ac7_scope_primary_view(SCOPE_READ, view, config.output_width, config.output_height)
}

/// Engine pool and resource references held for one queued pass until its end marker runs.
struct PassLease {
    owner: *const Renderer,
    next: *mut PassLease,
    input_pool: u64,
    depth_pool: u64,
    exposure_pool: u64,
    motion_pool: u64,
    ui_pool: u64,
    motion: *mut c_void,
    ui: *mut c_void,
    scene_surface: *mut c_void,
    output_pool: AtomicU64,
    uniforms: Vec<u64>,
}

impl PassLease {
    fn new(owner: &Renderer) -> Option<Box<Self>> {
        try_box(Self {
            owner,
            next: null_mut(),
            input_pool: 0,
            depth_pool: 0,
            exposure_pool: 0,
            motion_pool: 0,
            ui_pool: 0,
            motion: null_mut(),
            ui: null_mut(),
            scene_surface: null_mut(),
            output_pool: AtomicU64::new(0),
            uniforms: Vec::new(),
        })
    }
}

/// A lease of `lease` resolved by `resolve_pass` and retired by `release_pass`.
fn pass_lease(lease: *mut PassLease) -> rsf_ac7_scope_lease {
    rsf_ac7_scope_lease {
        object: lease.cast(),
        resolve: Some(resolve_pass),
        release: Some(release_pass),
    }
}

unsafe extern "C" fn resolve_pass(object: *mut c_void, scope: *mut rsf_ac7_render_scope) {
    let lease = unsafe { &*object.cast::<PassLease>() };
    let scope = unsafe { &mut *scope };
    let output = lease.output_pool.load(Ordering::Acquire);
    scope.color_input = pool_texture(lease.input_pool, false);
    scope.color_output = pool_texture(output, true);
    scope.color_output_readable = pool_texture(output, false);
    scope.depth = pool_texture(lease.depth_pool, false);
    scope.motion = if lease.motion_pool != 0 {
        pool_texture(lease.motion_pool, false)
    } else {
        lease.motion
    };
    scope.exposure = pool_texture(lease.exposure_pool, false);
    scope.ui_input = if lease.ui_pool != 0 {
        pool_texture(lease.ui_pool, false)
    } else {
        lease.ui
    };
    scope.scene_surface = lease.scene_surface;
}

/// Releases the lease's D3D11 references now and queues its engine references for the render
/// owner, which drains them (`drain_retired`).
unsafe extern "C" fn release_pass(object: *mut c_void) {
    let lease = object.cast::<PassLease>();
    let fields = unsafe { &mut *lease };
    pool_ref(fields.ui_pool, 0x30);
    for resource in [
        &mut fields.motion,
        &mut fields.ui,
        &mut fields.scene_surface,
    ] {
        let resource = std::mem::replace(resource, null_mut());
        unsafe { crate::com_resource::release(resource) };
    }
    let retired = unsafe { &(*fields.owner).retired };
    let mut head = retired.load(Ordering::SeqCst);
    loop {
        fields.next = head;
        match retired.compare_exchange_weak(head, lease, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => break,
            Err(current) => head = current,
        }
    }
}

fn current_thread() -> u32 {
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
}

/// Releases retired leases' engine references, only on the render owner's thread.
fn drain_retired(owner: &Renderer) {
    if owner.render_thread.load(Ordering::SeqCst) != current_thread() {
        return;
    }
    let mut lease = owner.retired.swap(null_mut(), Ordering::SeqCst);
    while !lease.is_null() {
        let mut owned = unsafe { Box::from_raw(lease) };
        lease = owned.next;
        pool_ref(owned.input_pool, 0x30);
        pool_ref(owned.depth_pool, 0x30);
        pool_ref(owned.exposure_pool, 0x30);
        pool_ref(owned.output_pool.load(Ordering::SeqCst), 0x30);
        pool_ref(owned.motion_pool, 0x30);
        for uniform in &mut owned.uniforms {
            release_uniform(uniform);
        }
    }
}

fn open(owner: &Renderer, list: *mut c_void, scope: &rsf_ac7_render_scope) -> Option<*mut c_void> {
    let mut ticket = null_mut();
    (unsafe { rsf_ac7_render_scope_open(owner.scopes(), list, scope, &mut ticket) } != 0)
        .then_some(ticket.cast())
}

fn open_leased(
    owner: &Renderer,
    list: *mut c_void,
    scope: &rsf_ac7_render_scope,
    lease: &rsf_ac7_scope_lease,
) -> Option<*mut c_void> {
    let mut ticket = null_mut();
    (unsafe { rsf_ac7_render_scope_open_leased(owner.scopes(), list, scope, lease, &mut ticket) }
        != 0)
        .then_some(ticket.cast())
}

fn enqueue_leased(
    owner: &Renderer,
    list: *mut c_void,
    scope: &rsf_ac7_render_scope,
    lease: &rsf_ac7_scope_lease,
) -> bool {
    unsafe { rsf_ac7_render_scope_enqueue_leased(owner.scopes(), list, scope, lease) != 0 }
}

fn close(ticket: *mut c_void, list: *mut c_void) -> bool {
    unsafe { rsf_ac7_render_scope_close(ticket.cast(), list) != 0 }
}

fn hook_role(hook: u32) -> u32 {
    rsf_ac7_hook_role(hook)
}

fn hook_rva(hook: u32) -> u32 {
    rsf_ac7_hook_rva(hook)
}
