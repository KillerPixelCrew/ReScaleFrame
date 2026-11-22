// SPDX-License-Identifier: MIT
//! The AC7 native graph fixture. It drives the native
//! renderer's private detours through an independent engine ABI caller and queued executor:
//! screen classification, particle tags and bindings, the temporal velocity patches, the SR graph
//! node with its spatial fallback on WARP, view preparation, the hook table, guards and the
//! TrueSky relays. tests/ac7_native_graph.c launches it with the replay helpers of
//! ac7_view_fixture.rs and the runtime's rsf_native_sr_evaluate.
use super::graph::{
    GraphPlan, NativeOutput, NativeRef, SrNode, hooked_context, hooked_process, make_plan,
    node_output, node_release, output_pool, set_postprocess,
};
use super::particles::{hooked_beam_fill, tag_beam_vertices, tag_trail_vertices};
use super::translucency::{ParticleBindingCommand, hooked_mesh_particle_parameters};
use super::views::{hooked_visibility, prepare_owned_views};
use super::{
    Guard, INSTALLED, Renderer, SITES, TEST_ENGINE, TEST_MODULE, TEST_TRUESKY_MODULE, copy,
    drain_retired, function, hook, key, method, pool_ref, read,
};
use crate::sys::RSF_AC7_VIEW_BUFFER_BYTES;
use crate::sys::{
    RSF_AC7_ROLE_OUTPUT, RSF_AC7_ROLE_SR, rsf_ac7_code_leaves, rsf_ac7_native_calls,
    rsf_ac7_truesky_patches,
};
use crate::sys::{rsf_ac7_native_renderer_options, rsf_ac7_render_scope, rsf_ac7_render_scope_fn};
use rsf_game_sys::rsf_game_render_config;
use std::cell::{Cell, OnceCell, UnsafeCell};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::mem::{offset_of, size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU32, Ordering};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_SAMPLE_DESC};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
};
use windows::core::Interface;

#[allow(dead_code)]
#[path = "ac7_graph_replay_abi.rs"]
mod replay_abi;

use replay_abi::{
    rsf_ac7_graph_check_fn, rsf_ac7_graph_particle_replay, rsf_ac7_graph_screen_fn,
    rsf_ac7_graph_sky_apply_fn, rsf_ac7_graph_sky_restore_fn,
};

/// The fixture helpers and runtime entry the C launcher links, passed in so this crate holds no
/// reference to symbols only the fixture executable defines.
#[repr(C)]
pub(super) struct Calls {
    replay_camera:
        unsafe extern "C" fn(*mut u8, usize, rsf_ac7_graph_screen_fn, rsf_ac7_graph_check_fn),
    replay_particles: unsafe extern "C" fn(*const rsf_ac7_graph_particle_replay),
    replay_truesky: unsafe extern "C" fn(
        *mut u8,
        usize,
        *mut c_void,
        *const rsf_ac7_truesky_patches,
        *mut c_void,
        rsf_ac7_graph_sky_apply_fn,
        rsf_ac7_graph_sky_restore_fn,
        rsf_ac7_graph_check_fn,
    ),
    fixture_view: unsafe extern "C" fn(*mut f32, usize) -> bool,
    velocity_patches: unsafe extern "C" fn(*mut u8, usize, *const rsf_ac7_code_leaves) -> bool,
    fixture_hooks: unsafe extern "C" fn(*const u32, usize, u32) -> bool,
    native_sr_evaluate: unsafe extern "C" fn(*mut c_void, *const rsf_ac7_render_scope) -> i32,
    /// rsf_ac7_render_scopes_create (render_scope_entry.c).
    scopes_create: unsafe extern "C" fn(
        u64,
        u32,
        rsf_ac7_render_scope_fn,
        *mut c_void,
        *mut *mut c_void,
    ) -> c_int,
    /// rsf_ac7_native_calls_table (native_renderer_platform.c).
    native: *const rsf_ac7_native_calls,
}

const PIPELINE_ERROR_NOT_RUNNING: i32 = -4;

static PASSED: AtomicBool = AtomicBool::new(true);

unsafe extern "C" fn check(ok: bool, why: *const c_char) {
    if !ok {
        PASSED.store(false, Ordering::SeqCst);
        eprintln!("{}", unsafe { CStr::from_ptr(why) }.to_string_lossy());
    }
}

fn verify(ok: bool, why: &CStr) {
    unsafe { check(ok, why.as_ptr()) };
}

static PARTICLE_SLOT: AtomicU32 = AtomicU32::new(0);
static PARTICLE_SHADER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static PARTICLE_PREVIOUS: AtomicPtr<c_void> = AtomicPtr::new(null_mut());
static BEAM_CALLS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn beam_original(_: *mut c_void, _: *mut c_void) -> u64 {
    BEAM_CALLS.fetch_add(1, Ordering::SeqCst);
    0x1234
}

unsafe extern "C" fn particle_original(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
) {
}

unsafe extern "C" fn particle_set(
    _: *mut c_void,
    shader: *mut c_void,
    slot: u32,
    previous: *mut c_void,
) {
    PARTICLE_SLOT.store(slot, Ordering::SeqCst);
    PARTICLE_SHADER.store(shader, Ordering::SeqCst);
    PARTICLE_PREVIOUS.store(previous, Ordering::SeqCst);
}

#[repr(C)]
struct Command {
    next: *mut Command,
    execute: unsafe extern "C" fn(*mut c_void, *mut Command),
}

#[repr(C)]
struct List {
    root: *mut Command,
    tail: *mut *mut Command,
    executing: u32,
    count: u32,
    uid: u32,
}

const _: () = assert!(offset_of!(List, count) == 0x14);

#[repr(C)]
struct Texture {
    table: *const *const c_void,
    native: *mut c_void,
}

#[repr(C)]
struct Pool {
    table: *const *const c_void,
    target: *mut Texture,
    shader: *mut Texture,
    refs: u32,
}

unsafe extern "C" fn add(pool: *mut Pool) -> u32 {
    unsafe {
        (*pool).refs += 1;
        (*pool).refs
    }
}

unsafe extern "C" fn drop_ref(pool: *mut Pool) -> u32 {
    unsafe {
        verify((*pool).refs > 0, c"native pool released twice");
        (*pool).refs = (*pool).refs.wrapping_sub(1);
        (*pool).refs
    }
}

unsafe extern "C" fn get_native(texture: *mut Texture) -> *mut c_void {
    unsafe { (*texture).native }
}

#[repr(C)]
struct Pass {
    table: *const *const c_void,
    flags: u64,
    reference: NativeRef,
    out: NativeOutput,
    auxiliary: NativeRef,
}

impl Pass {
    fn new(table: *const *const c_void) -> Self {
        let mut pass: Self = unsafe { zeroed() };
        pass.table = table;
        pass
    }
}

unsafe extern "C" fn get_input(pass: *mut Pass, index: u32) -> *mut c_void {
    unsafe {
        if index == 0 {
            (&raw mut (*pass).reference).cast()
        } else if index == 1 && (*pass).auxiliary.node != 0 {
            (&raw mut (*pass).auxiliary).cast()
        } else {
            null_mut()
        }
    }
}

unsafe extern "C" fn get_output(pass: *mut Pass, index: u32) -> *mut c_void {
    if index != 0 {
        null_mut()
    } else {
        unsafe { (&raw mut (*pass).out).cast() }
    }
}

unsafe extern "C" fn get_desc(pass: *mut Pass, out: *mut c_void, _: u32) -> *mut c_void {
    unsafe {
        let reference = (*pass).reference;
        let input = node_output(reference.node, reference.index);
        let source = if input.is_null() {
            (&raw const (*pass).out.descriptor).cast::<u8>()
        } else {
            (&raw const (*input).descriptor).cast::<u8>()
        };
        std::ptr::copy_nonoverlapping(source, out.cast::<u8>(), 0x50);
    }
    out
}

#[repr(C)]
struct Uniform {
    unused: u64,
    refs: AtomicI32,
    padding: u32,
}

#[repr(C)]
struct Clear {
    command: Command,
    color: [f32; 4],
}

/// Everything the engine stand-ins reach. The fixture runs on one thread.
struct Harness {
    list: UnsafeCell<List>,
    scene: UnsafeCell<[u8; 0x280]>,
    context: UnsafeCell<[u8; 0x60]>,
    view: UnsafeCell<[u8; 0x27c0]>,
    immediate: OnceCell<ID3D11DeviceContext>,
    target_view: OnceCell<ID3D11RenderTargetView>,
    output_pool: Cell<*mut Pool>,
    registered: Cell<*mut SrNode>,
    fallback_draws: Cell<u32>,
    sr_calls: Cell<u32>,
    aa_draws: Cell<u32>,
    uniform_rebuilds: Cell<u32>,
    family_rebuilds: Cell<u32>,
    configured_output: Cell<u32>,
    configured_width: Cell<u32>,
    configured_height: Cell<u32>,
    original_uniform: Uniform,
    generated_uniform: Uniform,
    observed_frame: Cell<u64>,
    auxiliary_done: Cell<bool>,
    dynamic_table: UnsafeCell<[*const c_void; 31]>,
    dynamic_object: UnsafeCell<*const *const c_void>,
    dynamic_pointer: UnsafeCell<*const c_void>,
    layout: UnsafeCell<u64>,
    native_sr_evaluate:
        Cell<Option<unsafe extern "C" fn(*mut c_void, *const rsf_ac7_render_scope) -> i32>>,
}

static HARNESS: AtomicPtr<Harness> = AtomicPtr::new(null_mut());

fn harness() -> &'static Harness {
    unsafe { &*HARNESS.load(Ordering::SeqCst) }
}

fn bump(counter: &Cell<u32>) {
    counter.set(counter.get() + 1);
}

fn view() -> *mut u8 {
    harness().view.get().cast()
}

fn scene_bytes() -> *mut u8 {
    harness().scene.get().cast()
}

fn rect(address: *const u8) -> [i32; 4] {
    let mut rect = [0i32; 4];
    copy(rect.as_mut_ptr().cast(), address.cast(), 16);
    rect
}

fn put<T: Copy>(address: *mut u8, value: T) {
    unsafe { address.cast::<T>().write_unaligned(value) };
}

unsafe extern "C" fn clear(_: *mut c_void, command: *mut Command) {
    let command = unsafe { Box::from_raw(command.cast::<Clear>()) };
    let harness = harness();
    if let (Some(immediate), Some(target)) = (harness.immediate.get(), harness.target_view.get()) {
        unsafe { immediate.ClearRenderTargetView(target, &command.color) };
    }
}

unsafe extern "C" fn spatial(raw_node: *mut c_void, raw_context: *mut c_void) {
    let harness = harness();
    verify(
        harness.auxiliary_done.get(),
        c"native auxiliary graph branches finish before SR size transition",
    );
    bump(&harness.fallback_draws);
    let node = raw_node.cast::<SrNode>();
    let current = rect(view().wrapping_add(0x70));
    verify(
        current[2] == 2 && current[3] == 2,
        c"fallback must read original source rectangle",
    );
    verify(
        unsafe { (*node).output.descriptor[0x2c] } == 10,
        c"SR graph keeps linear floating point output",
    );
    verify(
        raw_context == harness.context.get().cast(),
        c"native context forwarded",
    );
    unsafe { (*node).output.pool = key(harness.output_pool.get().cast()) };
    let command = Box::into_raw(Box::new(Clear {
        command: Command {
            next: null_mut(),
            execute: clear,
        },
        color: [0.0, 0.0, 1.0, 1.0],
    }))
    .cast::<Command>();
    let list = harness.list.get();
    unsafe {
        *(*list).tail = command;
        (*list).tail = &raw mut (*command).next;
        (*list).count += 1;
    }
}

unsafe extern "C" fn tone(raw: *mut c_void, _: *mut c_void) {
    let pass = raw.cast::<Pass>();
    unsafe {
        let reference = (*pass).reference;
        (*pass).out.pool = output_pool(reference.node, reference.index);
        pool_ref((*pass).out.pool, 0x28);
    }
}

unsafe extern "C" fn aa(_: *mut c_void, _: *mut c_void) {
    bump(&harness().aa_draws);
}

unsafe extern "C" fn nothing(_: *mut c_void, _: *mut c_void) {}

unsafe extern "C" fn auxiliary_process(_: *mut c_void, _: *mut c_void) {
    let current = rect(view().wrapping_add(0x70));
    verify(
        current[2] == 2 && current[3] == 2,
        c"bloom/grading producer keeps original native rectangle",
    );
    harness().auxiliary_done.set(true);
}

unsafe extern "C" fn get_scene() -> *mut c_void {
    scene_bytes().cast()
}

unsafe extern "C" fn register_node(graph: *mut c_void, node: *mut c_void) -> *mut c_void {
    let harness = harness();
    verify(
        graph == harness.context.get().cast::<u8>().wrapping_add(0x18).cast(),
        c"registration uses context graph owner",
    );
    verify(harness.registered.get().is_null(), c"one SR node per graph");
    harness.registered.set(node.cast());
    node
}

unsafe extern "C" fn remove_jitter(matrices: *mut c_void) {
    unsafe { matrices.cast::<u8>().add(0x360).write_bytes(0, 8) };
}

unsafe extern "C" fn parameters(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    count: u32,
    _: *mut c_void,
) {
    verify(
        count == 2,
        c"native uniform builder gets both lighting-volume bounds",
    );
    bump(&harness().uniform_rebuilds);
}

unsafe extern "C" fn create_uniform(
    _: *mut c_void,
    out: *mut u64,
    _: *const c_void,
    _: *const c_void,
    usage: u32,
) -> *mut c_void {
    verify(
        usage == 1,
        c"postprocess uniform is single-frame engine allocation",
    );
    unsafe { *out = key((&raw const harness().generated_uniform).cast()) };
    out.cast()
}

unsafe extern "C" fn move_uniform(destination: *mut c_void, source: *mut c_void) -> *mut c_void {
    unsafe {
        let previous = destination.cast::<u64>().read_unaligned();
        if previous != 0 {
            (*(previous as usize as *const Uniform))
                .refs
                .fetch_sub(1, Ordering::SeqCst);
        }
        destination
            .cast::<u64>()
            .write_unaligned(source.cast::<u64>().read_unaligned());
        source.cast::<u64>().write_unaligned(0);
    }
    destination
}

unsafe extern "C" fn family_size(_: *mut c_void) {
    bump(&harness().family_rebuilds);
}

unsafe extern "C" fn visibility(raw: *mut c_void, _: *mut c_void, third: usize, _: *mut c_void) {
    let mut storage = 0u64;
    read(key(raw), 0xb8, &mut storage);
    let (mut first, mut second) = (0u32, 0u32);
    read(storage, 0x13c0, &mut first);
    read(storage + 0x27c0, 0x13c0, &mut second);
    verify(
        first == 2 && second == 1 && third == 77,
        c"native jitter preparation selects main view and forwards engine arguments",
    );
}

unsafe extern "C" fn resolve_engine(rva: u32) -> *mut c_void {
    let harness = harness();
    match rva {
        0x109_dc70 => get_scene as *mut c_void,
        0xe9_3670 => register_node as *mut c_void,
        0xff_9af0 => remove_jitter as *mut c_void,
        0x113_46d0 => parameters as *mut c_void,
        0xde_5cf0 => move_uniform as *mut c_void,
        0x3c7_83d0 => harness.dynamic_pointer.get().cast(),
        0x3ca_a6c8 => harness.layout.get().cast(),
        0x19b_6fe0 => family_size as *mut c_void,
        0x121_83c0 => wait_recorders as *mut c_void,
        0xfb_7a00 => unmatched_downsample as *mut c_void,
        _ => {
            verify(false, c"unexpected native helper in adapter");
            null_mut()
        }
    }
}

unsafe extern "C" fn wait_recorders() {}

unsafe extern "C" fn unmatched_downsample() {}

unsafe extern "C" fn config(_: *mut c_void, config: *mut rsf_game_render_config) -> c_int {
    let harness = harness();
    unsafe {
        (*config).enabled = 1;
        (*config).output_width = harness.configured_output.get();
        (*config).output_height = harness.configured_output.get();
        (*config).render_width = harness.configured_width.get();
        (*config).render_height = harness.configured_height.get();
    }
    1
}

unsafe extern "C" fn notify(
    _: *mut c_void,
    _: *mut c_void,
    pass: *const rsf_ac7_render_scope,
    begin: u32,
) {
    let pass = unsafe { &*pass };
    if begin == 0 || pass.role != RSF_AC7_ROLE_SR {
        return;
    }
    let harness = harness();
    bump(&harness.sr_calls);
    harness.observed_frame.set(pass.native_frame);
    let output = unsafe { (*(*harness.output_pool.get()).target).native };
    verify(
        pass.color_output == output && !pass.depth.is_null() && !pass.motion.is_null(),
        c"resolved leased resources are current",
    );
    verify(
        pass.camera.render_width == 2 && pass.camera.output_width == 4,
        c"camera keeps pre-SR units",
    );
    let evaluate = harness.native_sr_evaluate.get().expect("native SR entry");
    let immediate = harness
        .immediate
        .get()
        .map_or(null_mut(), Interface::as_raw);
    verify(
        unsafe { evaluate(immediate, pass) } == PIPELINE_ERROR_NOT_RUNNING,
        c"a stopped backend refuses without touching the native spatial fallback",
    );
}

type GetPointer = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type Process = unsafe extern "C" fn(*mut c_void, *mut c_void);

/// The caller gathers child outputs first, writes each returned descriptor and runs native nodes.
fn gather(node: u64) {
    let dependency: GetPointer = unsafe { function(method(node, 0x40) as usize as *mut c_void) };
    for index in 0.. {
        let reference =
            unsafe { dependency(node as usize as *mut c_void, index) }.cast::<NativeRef>();
        if reference.is_null() {
            break;
        }
        let child = unsafe { (*reference).node };
        if child != 0 {
            gather(child);
        }
    }
    let out = node_output(node, 0);
    let mut descriptor = [0u8; 0x50];
    let describe: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> *mut c_void =
        unsafe { function(method(node, 0x70) as usize as *mut c_void) };
    unsafe {
        describe(
            node as usize as *mut c_void,
            descriptor.as_mut_ptr().cast(),
            0,
        );
        (*out).descriptor = descriptor;
    }
}

fn run() {
    let list = harness().list.get();
    unsafe {
        (*list).executing = 1;
        let mut command = (*list).root;
        while !command.is_null() {
            let next = (*command).next;
            ((*command).execute)(list.cast(), command);
            command = next;
        }
        (*list).executing = 0;
        (*list).root = null_mut();
        (*list).tail = &raw mut (*list).root;
        (*list).count = 0;
    }
}

fn execute_graph(node: u64, context: *mut c_void) {
    let bytes = node as usize as *mut u8;
    unsafe {
        if *bytes.add(9) != 0 {
            return;
        }
        *bytes.add(9) = 1;
    }
    let dependency: GetPointer = unsafe { function(method(node, 0x40) as usize as *mut c_void) };
    for index in 0.. {
        let reference =
            unsafe { dependency(node as usize as *mut c_void, index) }.cast::<NativeRef>();
        if reference.is_null() {
            break;
        }
        let child = unsafe { (*reference).node };
        if child != 0 {
            execute_graph(child, context);
        }
    }
    let process: Process = unsafe { function(method(node, 0x28) as usize as *mut c_void) };
    unsafe { process(node as usize as *mut c_void, context) };
}

unsafe extern "C" fn original_context(raw_context: *mut c_void, raw_root: *mut c_void) {
    gather(key(raw_root));
    execute_graph(key(raw_root), raw_context);
    let current = rect(view().wrapping_add(0x70));
    verify(
        current[2] == 4 && current[3] == 4,
        c"post-SR consumers get output rectangle in same graph",
    );
    let mut size = [0i32; 2];
    copy(
        size.as_mut_ptr().cast(),
        scene_bytes().wrapping_add(0x208).cast(),
        8,
    );
    verify(
        size == [4, 4],
        c"post-SR shader source size is native output size",
    );
    verify(
        harness().aa_draws.get() == 0,
        c"FXAA does not filter the temporal reconstruction again",
    );
}

fn options() -> rsf_ac7_native_renderer_options {
    unsafe { zeroed() }
}

fn new_renderer(options: &rsf_ac7_native_renderer_options) -> Box<Renderer> {
    Renderer::create(options).expect("allocate a fixture renderer")
}

fn allocate(size: usize) -> *mut u8 {
    unsafe { VirtualAlloc(None, size, MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE) }.cast()
}

fn free(memory: *mut u8) {
    let _ = unsafe { VirtualFree(memory.cast(), 0, MEM_RELEASE) };
}

unsafe extern "C" fn classify(reset: *mut u32, why: *mut u32, owner: *mut u64) -> u32 {
    unsafe { super::frame::render_screen(&mut *reset, &mut *why, &mut *owner) }
}

unsafe extern "C" fn tag_beam(source: *const c_void, fill: *const c_void) -> bool {
    tag_beam_vertices(source, fill)
}

unsafe extern "C" fn tag_trail(source: *const c_void, fill: *const c_void, kind: u32) -> bool {
    tag_trail_vertices(source, fill, kind)
}

fn set_original(hook: u32, address: *mut c_void) {
    SITES[hook as usize]
        .original
        .store(address, Ordering::SeqCst);
}

unsafe extern "C" fn beam_originals(
    owner: *mut c_void,
    source: *mut c_void,
    fill: *mut c_void,
) -> bool {
    INSTALLED.store(owner.cast(), Ordering::SeqCst);
    for beam in [
        hook::BEAM_NO_NOISE,
        hook::BEAM_NOISE,
        hook::BEAM_INTERPOLATED_NOISE,
    ] {
        set_original(beam, beam_original as *mut c_void);
    }
    let forwarded = unsafe {
        hooked_beam_fill::<{ hook::BEAM_NO_NOISE }>(source, fill) == 0x1234
            && hooked_beam_fill::<{ hook::BEAM_NOISE }>(source, fill) == 0x1234
            && hooked_beam_fill::<{ hook::BEAM_INTERPOLATED_NOISE }>(source, fill) == 0x1234
    } && BEAM_CALLS.load(Ordering::SeqCst) == 3;
    INSTALLED.store(null_mut(), Ordering::SeqCst);
    forwarded
}

unsafe extern "C" fn trail_originals(
    owner: *mut c_void,
    source: *mut c_void,
    fill: *mut c_void,
) -> bool {
    INSTALLED.store(owner.cast(), Ordering::SeqCst);
    for trail in [hook::RIBBON_FILL, hook::ANIM_TRAIL_FILL] {
        set_original(trail, beam_original as *mut c_void);
    }
    let forwarded = unsafe {
        hooked_beam_fill::<{ hook::RIBBON_FILL }>(source, fill) == 0x1234
            && hooked_beam_fill::<{ hook::ANIM_TRAIL_FILL }>(source, fill) == 0x1234
    } && BEAM_CALLS.load(Ordering::SeqCst) == 5;
    INSTALLED.store(null_mut(), Ordering::SeqCst);
    forwarded
}

fn replay_particles(calls: &Calls) {
    let owner = new_renderer(&options());
    owner.active.store(true, Ordering::SeqCst);
    let replay = rsf_ac7_graph_particle_replay {
        owner: owner.handle().cast(),
        check: Some(check),
        tag_beam: Some(tag_beam),
        tag_trail: Some(tag_trail),
        beam_originals: Some(beam_originals),
        trail_originals: Some(trail_originals),
    };
    unsafe { (calls.replay_particles)(&replay) };
}

#[repr(C)]
struct Reference {
    vtable: u64,
    refs: AtomicI32,
    pad: u32,
}

/// The actual native producer hook and its queued RHI command. References must survive producer
/// return, then retire on the render owner, not while commands are pending.
fn replay_particle_binding() {
    let shader_ref = Reference {
        vtable: 0,
        refs: AtomicI32::new(1),
        pad: 0,
    };
    let previous_ref = Reference {
        vtable: 0,
        refs: AtomicI32::new(1),
        pad: 0,
    };
    let mut shader = [0u8; 0x50];
    let mut resource = [0u8; 0x40];
    let mut factory = [0u8; 0x4a8];
    let mut command_list = [0u64; 0x30 / 8];
    let mut context_table = [null::<c_void>(); 0x1d8 / 8];
    context_table[0x1d0 / 8] = particle_set as *const c_void;
    let context_pointer = context_table.as_ptr();
    let context_address = &raw const context_pointer;
    put(
        shader.as_mut_ptr().wrapping_add(0x48),
        key(resource.as_ptr().cast()),
    );
    put(
        resource.as_mut_ptr().wrapping_add(0x38),
        key((&raw const shader_ref).cast()),
    );
    put(
        factory.as_mut_ptr().wrapping_add(0x4a0),
        key((&raw const previous_ref).cast()),
    );
    let list = command_list.as_mut_ptr().cast::<u8>();
    let root = list.cast::<*mut ParticleBindingCommand>();
    put(list.wrapping_add(8), root);
    put(list.wrapping_add(0x20), key(context_address.cast()));
    let owner = new_renderer(&options());
    owner.active.store(true, Ordering::SeqCst);
    INSTALLED.store((&raw const *owner).cast_mut(), Ordering::SeqCst);
    set_original(
        hook::MESH_PARTICLE_PARAMETERS,
        particle_original as *mut c_void,
    );
    unsafe {
        hooked_mesh_particle_parameters(
            null_mut(),
            list.cast(),
            shader.as_mut_ptr().cast(),
            factory.as_mut_ptr().cast(),
            null_mut(),
            null_mut(),
        )
    };
    let queued = unsafe { *root };
    verify(
        !queued.is_null()
            && owner.pending_particle_bindings.load(Ordering::SeqCst) == 1
            && shader_ref.refs.load(Ordering::SeqCst) == 2
            && previous_ref.refs.load(Ordering::SeqCst) == 2,
        c"native mesh-particle history is retained through queued binding",
    );
    if !queued.is_null() {
        unsafe { ((*queued).execute)(list.cast(), queued) };
    }
    verify(
        PARTICLE_SLOT.load(Ordering::SeqCst) == 127
            && PARTICLE_SHADER.load(Ordering::SeqCst) == (&raw const shader_ref).cast_mut().cast()
            && PARTICLE_PREVIOUS.load(Ordering::SeqCst)
                == (&raw const previous_ref).cast_mut().cast()
            && owner.pending_particle_bindings.load(Ordering::SeqCst) == 0
            && !owner.retired.load(Ordering::SeqCst).is_null(),
        c"native previous transforms bind on the RHI context with deferred retirement",
    );
    let retired = owner.retired.swap(null_mut(), Ordering::SeqCst);
    if !retired.is_null() {
        let retired = unsafe { Box::from_raw(retired) };
        for value in &retired.uniforms {
            unsafe {
                (*((*value as usize + 8) as *const AtomicI32)).fetch_sub(1, Ordering::SeqCst)
            };
        }
    }
    verify(
        shader_ref.refs.load(Ordering::SeqCst) == 1
            && previous_ref.refs.load(Ordering::SeqCst) == 1,
        c"queued transform/shader references retire exactly once",
    );
    INSTALLED.store(null_mut(), Ordering::SeqCst);
}

unsafe extern "C" fn sky_apply(raw: *mut c_void) {
    super::sky::apply_cloud_resolution_patch(unsafe { &*raw.cast::<Renderer>() });
}

unsafe extern "C" fn sky_restore(raw: *mut c_void) -> bool {
    super::sky::restore_truesky(unsafe { &*raw.cast::<Renderer>() })
}

fn texture_desc() -> D3D11_TEXTURE2D_DESC {
    D3D11_TEXTURE2D_DESC {
        Width: 4,
        Height: 4,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R16G16B16A16_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        ..Default::default()
    }
}

/// Runs the fixture. 0 passes, 1 fails and 77 skips without a WARP device.
///
/// # Safety
/// `calls` is the launcher's complete table.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_native_graph_fixture(calls: *const Calls) -> c_int {
    let Some(calls) = (unsafe { calls.as_ref() }) else {
        return 1;
    };
    unsafe { super::rsf_ac7_native_renderer_bind(calls.native) };
    {
        // Only this native leaf switches the module observed by the production classifier.
        const MODULE_SIZE: usize = 0x3cd_0000;
        let module = allocate(MODULE_SIZE);
        verify(!module.is_null(), c"allocate native class-cache fixture");
        if !module.is_null() {
            TEST_MODULE.store(module, Ordering::SeqCst);
            unsafe { (calls.replay_camera)(module, MODULE_SIZE, Some(classify), Some(check)) };
            TEST_MODULE.store(null_mut(), Ordering::SeqCst);
            free(module);
        }
    }
    replay_particles(calls);
    replay_particle_binding();
    const PATCH_IMAGE_SIZE: usize = 0x119_0000;
    let patch_image = allocate(PATCH_IMAGE_SIZE);
    verify(
        !patch_image.is_null(),
        c"allocate temporal velocity code fixture",
    );
    if patch_image.is_null() {
        return 1;
    }
    let patch_leaves = super::leaves::code_leaves(None, null_mut());
    verify(
        unsafe { (calls.velocity_patches)(patch_image, PATCH_IMAGE_SIZE, &patch_leaves) },
        c"temporal gate preflight, idempotence and guarded retirement",
    );
    free(patch_image);
    let (mut device, mut immediate) = (None::<ID3D11Device>, None::<ID3D11DeviceContext>);
    if unsafe {
        D3D11CreateDevice(
            None::<&windows::Win32::Graphics::Dxgi::IDXGIAdapter>,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut immediate),
        )
    }
    .is_err()
    {
        return 77;
    }
    let (Some(device), Some(immediate)) = (device, immediate) else {
        return 77;
    };
    let replayed = replay_graph(calls, &device, &immediate);
    if replayed != 0 {
        return replayed;
    }
    if PASSED.load(Ordering::SeqCst) { 0 } else { 1 }
}

fn replay_graph(calls: &Calls, device: &ID3D11Device, immediate: &ID3D11DeviceContext) -> c_int {
    let desc = texture_desc();
    let mut color = None::<ID3D11Texture2D>;
    verify(
        unsafe { device.CreateTexture2D(&desc, None, Some(&mut color)) }.is_ok(),
        c"create native output",
    );
    let Some(color) = color else {
        return 1;
    };
    let mut target_view = None::<ID3D11RenderTargetView>;
    verify(
        unsafe { device.CreateRenderTargetView(&color, None, Some(&mut target_view)) }.is_ok(),
        c"create native fallback target",
    );
    let harness = Box::new(Harness {
        list: UnsafeCell::new(List {
            root: null_mut(),
            tail: null_mut(),
            executing: 0,
            count: 0,
            uid: 1,
        }),
        scene: UnsafeCell::new([0; 0x280]),
        context: UnsafeCell::new([0; 0x60]),
        view: UnsafeCell::new([0; 0x27c0]),
        immediate: OnceCell::new(),
        target_view: OnceCell::new(),
        output_pool: Cell::new(null_mut()),
        registered: Cell::new(null_mut()),
        fallback_draws: Cell::new(0),
        sr_calls: Cell::new(0),
        aa_draws: Cell::new(0),
        uniform_rebuilds: Cell::new(0),
        family_rebuilds: Cell::new(0),
        configured_output: Cell::new(4),
        configured_width: Cell::new(2),
        configured_height: Cell::new(2),
        original_uniform: Uniform {
            unused: 0,
            refs: AtomicI32::new(1),
            padding: 0,
        },
        generated_uniform: Uniform {
            unused: 0,
            refs: AtomicI32::new(1),
            padding: 0,
        },
        observed_frame: Cell::new(0),
        auxiliary_done: Cell::new(false),
        dynamic_table: UnsafeCell::new([null(); 31]),
        dynamic_object: UnsafeCell::new(null()),
        dynamic_pointer: UnsafeCell::new(null()),
        layout: UnsafeCell::new(0),
        native_sr_evaluate: Cell::new(Some(calls.native_sr_evaluate)),
    });
    let harness: &'static Harness = Box::leak(harness);
    unsafe {
        let list = harness.list.get();
        (*list).tail = &raw mut (*list).root;
        *harness.dynamic_object.get() = harness.dynamic_table.get().cast();
        *harness.dynamic_pointer.get() = harness.dynamic_object.get().cast();
        (*harness.dynamic_table.get())[30] = create_uniform as *const c_void;
    }
    let _ = harness.immediate.set(immediate.clone());
    if let Some(target_view) = target_view {
        let _ = harness.target_view.set(target_view);
    }
    HARNESS.store((harness as *const Harness).cast_mut(), Ordering::SeqCst);
    let mut texture_table = [null::<c_void>(); 7];
    let mut pool_table = [null::<c_void>(); 7];
    texture_table[6] = get_native as *const c_void;
    pool_table[5] = add as *const c_void;
    pool_table[6] = drop_ref as *const c_void;
    let mut texture = Texture {
        table: texture_table.as_ptr(),
        native: color.as_raw(),
    };
    let pool = |texture: *mut Texture| Pool {
        table: pool_table.as_ptr(),
        target: texture,
        shader: texture,
        refs: 1,
    };
    let (mut input, mut depth, mut output) =
        (pool(&mut texture), pool(&mut texture), pool(&mut texture));
    harness.output_pool.set(&mut output);
    let mut options = options();
    options.session_id = 7;
    options.render_config = Some(config);
    let renderer = new_renderer(&options);
    renderer
        .render_thread
        .store(super::current_thread(), Ordering::SeqCst);
    renderer.active.store(true, Ordering::SeqCst);
    let mut scopes = null_mut();
    verify(
        unsafe { (calls.scopes_create)(7, 32, Some(notify), null_mut(), &mut scopes) } != 0,
        c"create execution scopes",
    );
    renderer.scopes.store(scopes, Ordering::SeqCst);
    INSTALLED.store((&raw const *renderer).cast_mut(), Ordering::SeqCst);
    TEST_ENGINE.store(resolve_engine as *mut c_void, Ordering::SeqCst);
    let originals: [Process; 6] = [tone, aa, nothing, nothing, nothing, spatial];
    // Taken once, so the site targets and node tables hold the same detour addresses.
    let wrappers: [*mut c_void; 6] = [
        hooked_process::<{ hook::PROCESS_TONEMAP }> as *mut c_void,
        hooked_process::<{ hook::PROCESS_AA }> as *mut c_void,
        hooked_process::<{ hook::PROCESS_MATERIAL }> as *mut c_void,
        hooked_process::<{ hook::PROCESS_HUD }> as *mut c_void,
        hooked_process::<{ hook::PROCESS_COMPOSITE }> as *mut c_void,
        hooked_process::<{ hook::PROCESS_OUTPUT }> as *mut c_void,
    ];
    for index in 0..6u32 {
        let site = &SITES[(hook::PROCESS_TONEMAP + index) as usize];
        site.target
            .store(wrappers[index as usize], Ordering::SeqCst);
        site.original
            .store(originals[index as usize] as *mut c_void, Ordering::SeqCst);
        set_original(hook::DESCRIPTOR_TONEMAP + index, get_desc as *mut c_void);
    }
    set_original(hook::CONTEXT, original_context as *mut c_void);
    let node_table = || {
        let mut table = [null::<c_void>(); 17];
        table[1] = get_input as *const c_void;
        table[2] = table[1];
        table[8] = get_input as *const c_void;
        table[7] = get_output as *const c_void;
        table[14] = get_desc as *const c_void;
        table
    };
    let (mut source_table, mut tone_table, mut aa_table, mut auxiliary_table) =
        (node_table(), node_table(), node_table(), node_table());
    tone_table[5] = wrappers[0];
    aa_table[5] = wrappers[1];
    source_table[5] = nothing as *const c_void;
    auxiliary_table[5] = auxiliary_process as *const c_void;
    let mut source = Pass::new(source_table.as_ptr());
    let mut tonemap = Pass::new(tone_table.as_ptr());
    let mut anti_alias = Pass::new(aa_table.as_ptr());
    let mut auxiliary = Pass::new(auxiliary_table.as_ptr());
    source.out.pool = key((&raw mut input).cast());
    let internal = [2i32, 2];
    put(
        source.out.descriptor.as_mut_ptr().wrapping_add(0x14),
        internal,
    );
    auxiliary.out.descriptor = source.out.descriptor;
    auxiliary.out.pool = source.out.pool;
    tonemap.auxiliary.node = key((&raw mut auxiliary).cast());
    tonemap.reference.node = key((&raw mut source).cast());
    anti_alias.reference.node = key((&raw mut tonemap).cast());
    let mut fixture = [0f32; RSF_AC7_VIEW_BUFFER_BYTES as usize / size_of::<f32>()];
    if !unsafe { (calls.fixture_view)(fixture.as_mut_ptr(), size_of::<[f32; 1024]>()) } {
        return 1;
    }
    let cached = key(fixture.as_ptr().cast());
    let family = key(scene_bytes().cast());
    let frame = 41u32;
    let state = 0x12_3456u64;
    let view = view();
    let scene = scene_bytes();
    let old_uniform = key((&raw const harness.original_uniform).cast());
    put(view.wrapping_add(0x10), old_uniform);
    put(view, family);
    put(view.wrapping_add(8), state);
    let (render, full) = ([0i32, 0, 2, 2], [0i32, 0, 4, 4]);
    put(view.wrapping_add(0x70), render);
    put(view.wrapping_add(0x80), full);
    put(view.wrapping_add(0x90), full);
    put(view.wrapping_add(0x1418), cached);
    put(scene.wrapping_add(0x68), frame);
    put(scene.wrapping_add(0x60), key((&raw mut depth).cast()));
    put(scene.wrapping_add(0x208), internal);
    let context = harness.context.get().cast::<u8>();
    put(context, key(view.cast()));
    put(context.wrapping_add(0x28), key(harness.list.get().cast()));
    set_postprocess(key(view.cast()), key((&raw mut input).cast()));
    unsafe { hooked_context(context.cast(), (&raw mut anti_alias).cast()) };
    let registered = harness.registered.get();
    verify(
        !registered.is_null() && harness.fallback_draws.get() == 1,
        c"graph has native SR node and complete spatial fallback",
    );
    let mut buffer = [0i32; 2];
    copy(
        buffer.as_mut_ptr().cast(),
        scene.wrapping_add(0x208).cast(),
        8,
    );
    verify(
        rect(view.wrapping_add(0x70)) == render && buffer == internal,
        c"CPU view/scene sizes restored after graph construction",
    );
    let mut restored_uniform = 0u64;
    copy(
        (&raw mut restored_uniform).cast(),
        view.wrapping_add(0x10).cast(),
        8,
    );
    verify(
        harness.uniform_rebuilds.get() == 1 && restored_uniform == old_uniform,
        c"only shader parameters rebuilt; original native uniform ownership restored",
    );
    if registered.is_null() {
        return 1;
    }
    // Native graph retirement happens before its queued RHI work runs.
    unsafe { node_release(registered) };
    harness.registered.set(null_mut());
    verify(
        output.refs >= 2,
        c"queued pool leases survive native graph retirement",
    );
    put(scene.wrapping_add(0x68), 900u32);
    run();
    drain_retired(&renderer);
    let read_desc = D3D11_TEXTURE2D_DESC {
        BindFlags: 0,
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..desc
    };
    let mut readback = None::<ID3D11Texture2D>;
    verify(
        unsafe { device.CreateTexture2D(&read_desc, None, Some(&mut readback)) }.is_ok(),
        c"create fallback readback",
    );
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    match readback {
        Some(readback) => {
            unsafe { immediate.CopyResource(&readback, &color) };
            if unsafe { immediate.Map(&readback, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.is_ok()
            {
                let pixel = unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u16>(), 4) };
                verify(
                    pixel == [0, 0, 0x3c00, 0x3c00],
                    c"refused SR preserves the native blue fallback pixels",
                );
                unsafe { immediate.Unmap(&readback, 0) };
            } else {
                verify(false, c"read fallback pixels");
            }
        }
        None => verify(false, c"read fallback pixels"),
    }
    pool_ref(tonemap.out.pool, 0x30);
    pool_ref(anti_alias.out.pool, 0x30);
    verify(
        harness.sr_calls.get() == 1 && harness.observed_frame.get() == 41,
        c"delayed RHI execution keeps original frame identity",
    );
    verify(
        input.refs == 1 && depth.refs == 1,
        c"lease refs retired on original render owner",
    );
    let mut refused = GraphPlan::EMPTY;
    verify(
        !make_plan(
            &renderer,
            context.cast(),
            (&raw mut source).cast(),
            &mut refused,
        ),
        c"unknown graph path refuses without guessing texture roles",
    );
    let mut owned_renderer = [0u8; 0x220];
    let mut owned_views = vec![0u8; 0x27c0 * 2];
    let views = owned_views.as_mut_ptr();
    put(
        owned_renderer.as_mut_ptr().wrapping_add(0xb8),
        key(views.cast()),
    );
    put(owned_renderer.as_mut_ptr().wrapping_add(0xc0), 2i32);
    let (primary, secondary) = ([0i32, 0, 16, 16], [0i32, 0, 8, 8]);
    put(views.wrapping_add(0x80), primary);
    put(views.wrapping_add(0x27c0 + 0x80), secondary);
    put(views.wrapping_add(0x70), primary);
    put(views.wrapping_add(0x27c0 + 0x70), secondary);
    for offset in [0usize, 0x27c0] {
        put(views.wrapping_add(offset + 8), state);
        put(views.wrapping_add(offset + 0x13c0), 1u32);
        let unconstrained = rect(views.wrapping_add(offset + 0x80));
        put(views.wrapping_add(offset + 0x90), unconstrained);
    }
    let desired = rsf_game_render_config {
        struct_size: size_of::<rsf_game_render_config>() as u32,
        enabled: 1,
        output_width: 16,
        output_height: 16,
        render_width: 7,
        render_height: 5,
    };
    prepare_owned_views(owned_renderer.as_mut_ptr().cast(), &desired);
    let adjusted = rect(views.wrapping_add(0x70));
    verify(
        adjusted[2] == 7 && adjusted[3] == 5 && harness.family_rebuilds.get() == 1,
        c"renderer producer keeps exact backend rectangle before recomputing family allocation extent",
    );
    verify(
        rect(views.wrapping_add(0x27c0 + 0x70)) == secondary,
        c"secondary view retains its producer resolution",
    );
    put(scene.wrapping_add(0x208), [8i32, 8]);
    harness.configured_output.set(16);
    harness.configured_width.set(7);
    harness.configured_height.set(5);
    set_original(hook::VISIBILITY, visibility as *mut c_void);
    unsafe {
        hooked_visibility(
            owned_renderer.as_mut_ptr().cast(),
            null_mut(),
            77,
            null_mut(),
        )
    };
    let mut restored_mode = 0u32;
    copy(
        (&raw mut restored_mode).cast(),
        views.wrapping_add(0x13c0).cast(),
        4,
    );
    verify(
        restored_mode == 1,
        c"native AA graph choice restored after jitter producer",
    );
    harness.configured_output.set(4);
    harness.configured_width.set(2);
    harness.configured_height.set(2);
    // Pass the Rust hook indices, so the fixture checks them against the site table.
    let fixture_hooks = [
        hook::ENGINE_TICK,
        hook::BASE_PASS,
        hook::TARGET_INIT,
        hook::PROCESS_OUTPUT,
        hook::DESCRIPTOR_OUTPUT,
        hook::PIXEL_ENQUEUE_D,
        hook::CREATE_PIXEL_SHADER,
        hook::RHI_PIXEL_UNIFORM,
        hook::PIXEL_VIEW_UNIFORM,
        hook::SKY_PROJECTION,
    ];
    verify(
        unsafe {
            (calls.fixture_hooks)(
                fixture_hooks.as_ptr(),
                fixture_hooks.len(),
                RSF_AC7_ROLE_OUTPUT,
            )
        },
        c"hook table and diagnostic admission match the native enum",
    );
    // Hook guards count per thread and sum for the stop path.
    let held = crate::guards::held;
    verify(held(true) == 0, c"no hook body is running");
    {
        let _entry = Guard::entry();
        let _outer = Guard::outer();
        verify(
            held(false) == 1 && held(true) == 2,
            c"entry and outer guards are summed separately",
        );
    }
    verify(held(true) == 0, c"guards release their slot");
    // TrueSky relays (truesky.rs) through the native leaves on a writable plugin image. The depth
    // route's code and the shipped effect are absent, so it refuses and only the composite_tile
    // relay installs; restore puts the native call back and releases the relay memory.
    const PLUGIN_SIZE: usize = 0xd_0000;
    let plugin = allocate(PLUGIN_SIZE);
    verify(!plugin.is_null(), c"allocate a TrueSky code image");
    if !plugin.is_null() {
        let owner = new_renderer(&self::options());
        TEST_TRUESKY_MODULE.store(plugin, Ordering::SeqCst);
        unsafe {
            (calls.replay_truesky)(
                plugin,
                PLUGIN_SIZE,
                (&raw const *owner).cast_mut().cast(),
                owner.truesky.get(),
                super::sky::composite_draw_handler(),
                Some(sky_apply),
                Some(sky_restore),
                Some(check),
            )
        };
        TEST_TRUESKY_MODULE.store(null_mut(), Ordering::SeqCst);
        free(plugin);
    }
    unsafe { super::rsf_ac7_render_scopes_quiesce(renderer.scopes()) };
    verify(
        unsafe { super::rsf_ac7_render_scopes_destroy(renderer.scopes()) } != 0,
        c"quiescent graph unload",
    );
    INSTALLED.store(null_mut(), Ordering::SeqCst);
    TEST_ENGINE.store(null_mut(), Ordering::SeqCst);
    0
}
