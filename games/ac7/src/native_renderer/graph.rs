// SPDX-License-Identifier: MIT
//! The post-process graph: the pre-tonemap SR node and its plan, the reviewed pass Process and
//! descriptor hooks, the final scene and velocity scopes, and the generated view uniforms.
use super::frame::renderer_identity;
use super::{
    Guard, PROCESS_HOOKS, PassLease, Renderer, UniformOwner, at, close, copy, drain_retired,
    engine, engine_add_ref, engine_address, enqueue_leased, hook, hook_role, installed, key,
    method, move_uniform, native_texture, open_leased, original, pass_lease, pool_ref,
    pool_texture, primary_view, read, release_pass, release_uniform, scope, site_target,
};
use crate::sys::rsf_ac7_render_scope;
use crate::sys::rsf_ac7_scope_lease;
use crate::sys::{RSF_AC7_ROLE_SR, RSF_AC7_ROLE_TONEMAP};
use crate::sys::{RSF_AC7_VIEW_ABI_VERSION, RSF_AC7_VIEW_BUFFER_BYTES, rsf_ac7_view};
use crate::view::rsf_ac7_view_read;
use rsf_game_sys::ffi_util::{ascii_wide, try_box};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_FINAL_SCENE, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_UI_COMPOSITE, RSF_GAME_RENDER_VELOCITY_FINALIZE,
    rsf_camera_frame,
};
use std::cell::Cell;
use std::ffi::c_void;
use std::mem::{offset_of, size_of, zeroed};
use std::ptr::{copy_nonoverlapping, null_mut, write_bytes, write_unaligned};
use std::sync::atomic::{AtomicU32, Ordering};

const VIEW_OK: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct NativeRef {
    pub(super) node: u64,
    pub(super) index: u32,
    padding: u32,
}

impl NativeRef {
    const EMPTY: Self = Self {
        node: 0,
        index: 0,
        padding: 0,
    };
}

#[repr(C, align(8))]
pub(super) struct NativeOutput {
    pub(super) descriptor: [u8; 0x50],
    pub(super) pool: u64,
    dependencies: u32,
    padding: u32,
}

const _: () = assert!(
    size_of::<NativeRef>() == 0x10
        && size_of::<NativeOutput>() == 0x60
        && offset_of!(NativeOutput, pool) == 0x50
);

type GetPointer = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type Process = unsafe extern "C" fn(*mut c_void, *mut c_void);

pub(super) fn input_ref(node: u64, index: u32) -> *mut NativeRef {
    let entry = method(node, 8);
    if entry == 0 {
        return null_mut();
    }
    let call: GetPointer = unsafe { super::function(at(entry)) };
    unsafe { call(at(node), index) }.cast()
}

pub(super) fn node_output(node: u64, index: u32) -> *mut NativeOutput {
    let entry = method(node, 0x38);
    if entry == 0 {
        return null_mut();
    }
    let call: GetPointer = unsafe { super::function(at(entry)) };
    unsafe { call(at(node), index) }.cast()
}

/// Calls `visit` for each dependency of a graph node until it returns false. True only when the
/// node lists all of its (at most 16) dependencies and `visit` took every one.
fn for_each_dependency(node: u64, mut visit: impl FnMut(NativeRef) -> bool) -> bool {
    let entry = method(node, 0x40);
    if entry == 0 {
        return false;
    }
    let call: GetPointer = unsafe { super::function(at(entry)) };
    for index in 0..=16 {
        let reference = unsafe { call(at(node), index) }.cast::<NativeRef>();
        if reference.is_null() {
            return true;
        }
        if !visit(unsafe { *reference }) {
            return false;
        }
    }
    false
}

pub(super) fn output_pool(node: u64, index: u32) -> u64 {
    let mut pool = 0u64;
    read(
        key(node_output(node, index).cast()),
        offset_of!(NativeOutput, pool),
        &mut pool,
    );
    pool
}

fn retain_generated_uniform(owner: &Renderer, view: u64, uniform: u64) -> bool {
    let mut family = 0u64;
    if uniform == 0 || !read(view, 0, &mut family) || family < 0x10 {
        return false;
    }
    let mut tables = owner.tables();
    let Some(slot) = tables
        .uniform_owners
        .iter_mut()
        .find(|entry| entry.uniform == 0)
    else {
        return false;
    };
    engine_add_ref(uniform);
    *slot = UniformOwner {
        renderer: family - 0x10,
        view,
        uniform,
    };
    owner.pending_uniforms.fetch_add(1, Ordering::SeqCst);
    true
}

pub(super) fn retire_renderer_uniforms(owner: &Renderer, renderer: *mut c_void, list: *mut c_void) {
    let renderer = key(renderer);
    let mut tables = owner.tables();
    let owned = |entry: &UniformOwner| entry.renderer == renderer && entry.uniform != 0;
    let count = tables
        .uniform_owners
        .iter()
        .filter(|entry| owned(entry))
        .count();
    if count == 0 {
        return;
    }
    let Some(mut lease) = PassLease::new(owner) else {
        owner.deactivate();
        return;
    };
    if lease.uniforms.try_reserve(count).is_err() {
        drop(lease);
        drop(tables);
        owner.deactivate();
        owner.log(c"AC7 uniform retirement failed; generated buffers and module retained");
        return;
    }
    let mut retired = scope();
    retired.session_id = owner.options.session_id;
    retired.family_key = renderer.wrapping_add(0x10);
    for entry in tables.uniform_owners.iter().filter(|entry| owned(entry)) {
        retired.view_key = entry.view;
        lease.uniforms.push(entry.uniform);
    }
    // Native retirement waits for CPU recording tasks and dispatches their lists. A marker
    // appended afterward retires raw uniform pointers only after earlier RHI commands execute.
    // The scope carries numeric identity only; no retired FViewInfo memory is read here.
    let lease = Box::into_raw(lease);
    let lease_owner = rsf_ac7_scope_lease {
        object: lease.cast(),
        resolve: None,
        release: Some(release_pass),
    };
    if !enqueue_leased(owner, list, &retired, &lease_owner) {
        drop(unsafe { Box::from_raw(lease) });
        owner.deactivate();
        owner.log(c"AC7 uniform retirement refused; generated buffers and module retained");
        return;
    }
    for entry in tables
        .uniform_owners
        .iter_mut()
        .filter(|entry| owned(entry))
    {
        *entry = UniformOwner {
            renderer: 0,
            view: 0,
            uniform: 0,
        };
        owner.pending_uniforms.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The SR graph node in the engine's FRenderingCompositePass layout, read through `SR_TABLE`.
#[repr(C)]
pub(super) struct SrNode {
    table: *const *const c_void,
    flags: [u8; 8],
    inputs: [NativeRef; 2],
    pub(super) output: NativeOutput,
    unused: [u8; 0x28],
    quality: u32,
    panini: [f32; 3],
    extent: [i32; 2],
    plan: *mut GraphPlan,
    auxiliary: [NativeRef; 16],
    auxiliary_count: u32,
    exposure_ref: NativeRef,
    preceding_exposure_pool: u64,
}

const _: () = assert!(offset_of!(SrNode, output) == 0x30 && offset_of!(SrNode, quality) == 0xb8);

impl SrNode {
    fn new() -> Self {
        Self {
            table: SR_TABLE.0.as_ptr(),
            flags: [0; 8],
            inputs: [NativeRef::EMPTY; 2],
            output: NativeOutput {
                descriptor: [0; 0x50],
                pool: 0,
                dependencies: 0,
                padding: 0,
            },
            unused: [0; 0x28],
            quality: 1,
            panini: [0.0, 0.0, 1.0],
            extent: [0; 2],
            plan: null_mut(),
            auxiliary: [NativeRef::EMPTY; 16],
            auxiliary_count: 0,
            exposure_ref: NativeRef::EMPTY,
            preceding_exposure_pool: 0,
        }
    }
}

const MAX_CONSUMERS: usize = 64;

/// CPU-only state of one graph construction. Queued packets carry copies and pool leases, never
/// a pointer to it.
pub(super) struct GraphPlan {
    pub(super) owner: *const Renderer,
    pub(super) packet: rsf_ac7_render_scope,
    velocity: u64,
    depth_pool: u64,
    consumers: [u64; MAX_CONSUMERS],
    consumer_count: usize,
    saved_rect: [i32; 4],
    saved_buffer: [i32; 2],
    saved_matrices: [u8; 0x380],
    saved_parameters: [u8; 0xcf0],
    pub(super) cached_parameters: u64,
    pub(super) saved_uniform: u64,
    pub(super) scene: *mut c_void,
    pub(super) resized: bool,
    pub(super) surface_extent: [i32; 2],
}

impl GraphPlan {
    pub(super) const EMPTY: Self = Self {
        owner: std::ptr::null(),
        packet: unsafe { zeroed() },
        velocity: 0,
        depth_pool: 0,
        consumers: [0; MAX_CONSUMERS],
        consumer_count: 0,
        saved_rect: [0; 4],
        saved_buffer: [0; 2],
        saved_matrices: [0; 0x380],
        saved_parameters: [0; 0xcf0],
        cached_parameters: 0,
        saved_uniform: 0,
        scene: null_mut(),
        resized: false,
        surface_extent: [0; 2],
    };

    pub(super) fn contains(&self, pass: u64) -> bool {
        self.consumers[..self.consumer_count].contains(&pass)
    }

    fn owner(&self) -> &Renderer {
        unsafe { &*self.owner }
    }
}

thread_local! {
    pub(super) static GRAPH_PLAN: Cell<*mut GraphPlan> = const { Cell::new(null_mut()) };
    pub(super) static POSTPROCESS_VIEW: Cell<u64> = const { Cell::new(0) };
    static POSTPROCESS_VELOCITY: Cell<u64> = const { Cell::new(0) };
}

#[cfg(feature = "warp-fixture")]
pub(super) fn set_postprocess(view: u64, velocity: u64) {
    POSTPROCESS_VIEW.set(view);
    POSTPROCESS_VELOCITY.set(velocity);
}

pub(super) unsafe extern "C" fn node_release(node: *mut SrNode) {
    pool_ref(unsafe { (*node).output.pool }, 0x30);
    drop(unsafe { Box::from_raw(node) });
}

unsafe extern "C" fn node_destruct(node: *mut SrNode, flags: u32) -> *mut c_void {
    if flags & 1 != 0 {
        unsafe { node_release(node) };
    }
    node.cast()
}

unsafe extern "C" fn node_input(node: *mut SrNode, index: u32) -> *mut c_void {
    if index < 2 {
        unsafe { (&raw mut (*node).inputs[index as usize]).cast() }
    } else {
        null_mut()
    }
}

unsafe extern "C" fn node_set(node: *mut SrNode, index: u32, reference: *const NativeRef) {
    if index < 2 && !reference.is_null() {
        unsafe { (*node).inputs[index as usize] = *reference };
    }
}

unsafe extern "C" fn node_add(_: *mut SrNode, _: *const NativeRef) {}

unsafe extern "C" fn node_blends(_: *mut SrNode) -> bool {
    false
}

unsafe extern "C" fn sr_output(node: *mut SrNode, index: u32) -> *mut c_void {
    if index != 0 {
        null_mut()
    } else {
        unsafe { (&raw mut (*node).output).cast() }
    }
}

unsafe extern "C" fn node_dependency(node: *mut SrNode, index: u32) -> *mut c_void {
    let count = unsafe { (*node).auxiliary_count };
    if index == 0 {
        unsafe { (&raw mut (*node).inputs[0]).cast() }
    } else if index <= count {
        unsafe { (&raw mut (*node).auxiliary[index as usize - 1]).cast() }
    } else {
        null_mut()
    }
}

unsafe extern "C" fn node_none(_: *mut SrNode, _: u32) -> *mut c_void {
    null_mut()
}

/// The writable empty name the former static array provided.
struct EmptyString(std::cell::UnsafeCell<[u64; 2]>);
unsafe impl Sync for EmptyString {}
static EMPTY_STRING: EmptyString = EmptyString(std::cell::UnsafeCell::new([0; 2]));

unsafe extern "C" fn node_string(_: *mut SrNode, _: u32) -> *mut c_void {
    EMPTY_STRING.0.get().cast()
}

unsafe extern "C" fn node_set_string(_: *mut SrNode, _: u32, _: *const c_void) {}

unsafe extern "C" fn node_fence(_: *mut SrNode) -> *mut c_void {
    null_mut()
}

static SR_NAME: [u16; 15] = ascii_wide(b"ReScaleFrameSR\0");

unsafe extern "C" fn node_descriptor(
    node: *mut SrNode,
    result: *mut c_void,
    _: u32,
) -> *mut c_void {
    let (input, extent) = unsafe { ((*node).inputs[0], (*node).extent) };
    let source = node_output(input.node, input.index);
    let bytes = result.cast::<u8>();
    if source.is_null() {
        unsafe { write_bytes(bytes, 0, 0x50) };
        return result;
    }
    unsafe {
        copy_nonoverlapping((&raw const (*source).descriptor).cast::<u8>(), bytes, 0x50);
        write_unaligned(bytes.add(0x14).cast::<[i32; 2]>(), extent);
        // PF_FloatRGBA, TexCreate_RenderTargetable.
        write_unaligned(bytes.add(0x2c).cast::<u32>(), 10);
        write_unaligned(bytes.add(0x34).cast::<u32>(), 1);
        write_unaligned(bytes.add(0x28).cast::<u16>(), 1);
        *bytes.add(0x38) = 0;
        *bytes.add(0x48) = 1;
        *bytes.add(0x49) = 0;
        write_unaligned(bytes.add(0x40).cast::<*const u16>(), SR_NAME.as_ptr());
    }
    result
}

struct VTable([*const c_void; 17]);
unsafe impl Sync for VTable {}

static SR_TABLE: VTable = VTable([
    node_destruct as *const c_void,
    node_input as *const c_void,
    node_input as *const c_void,
    node_set as *const c_void,
    node_add as *const c_void,
    node_process as *const c_void,
    node_blends as *const c_void,
    sr_output as *const c_void,
    node_dependency as *const c_void,
    node_none as *const c_void,
    node_string as *const c_void,
    node_set_string as *const c_void,
    node_none as *const c_void,
    node_set_string as *const c_void,
    node_descriptor as *const c_void,
    node_release as *const c_void,
    node_fence as *const c_void,
]);

pub(super) fn wait_for_view_recorders() {
    // Native WaitForOutstandingTasksOnly joins CPU command recording before shared view edits.
    unsafe { engine::<unsafe extern "C" fn()>(0x121_83c0)() };
}

pub(super) fn retain_view_uniform(view: *mut u8, offset: usize) -> u64 {
    let mut uniform = 0u64;
    copy(
        (&mut uniform as *mut u64).cast(),
        view.wrapping_add(offset).cast(),
        size_of::<u64>(),
    );
    if uniform != 0 {
        engine_add_ref(uniform);
    }
    uniform
}

#[repr(C, align(16))]
struct Bounds([u8; 0x40]);

fn build_postprocess_uniform(plan: &mut GraphPlan, view: *mut u8) {
    let mut bounds = Bounds([0; 0x40]);
    // Invoke the pure parameter producer, not InitRHIResources' dynamic-resource initialization.
    type Parameters = unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        u32,
        *mut c_void,
    );
    unsafe {
        engine::<Parameters>(0x113_46d0)(
            view.cast(),
            plan.scene,
            view.wrapping_add(0xc0).cast(),
            view.wrapping_add(0x21e0).cast(),
            (&mut bounds as *mut Bounds).cast(),
            2,
            at(plan.cached_parameters),
        )
    };
    let (mut dynamic, mut uniform) = (0u64, 0u64);
    copy(
        (&mut dynamic as *mut u64).cast(),
        engine_address(0x3c7_83d0),
        8,
    );
    let create = method(dynamic, 0xf0);
    if create == 0 {
        return;
    }
    type CreateUniform = unsafe extern "C" fn(
        *mut c_void,
        *mut u64,
        *const c_void,
        *const c_void,
        u32,
    ) -> *mut c_void;
    let create: CreateUniform = unsafe { super::function(at(create)) };
    unsafe {
        create(
            at(dynamic),
            &mut uniform,
            at(plan.cached_parameters),
            engine_address(0x3ca_a6c8),
            1,
        )
    };
    if uniform == 0 {
        return;
    }
    if !retain_generated_uniform(plan.owner(), plan.packet.view_key, uniform) {
        release_uniform(&mut uniform);
        plan.owner()
            .log(c"AC7 generated view uniform refused: renderer lifetime capacity unavailable");
        return;
    }
    // Save an owned reference without clearing the shared slot. A worker can queue that slot.
    // Move only from the local replacement, so publication never exposes an intermediate null.
    plan.saved_uniform = retain_view_uniform(view, 0x10);
    move_uniform(
        view.wrapping_add(0x10).cast(),
        (&mut uniform as *mut u64).cast(),
    );
}

pub(super) fn resize_consumers(plan: &mut GraphPlan) {
    wait_for_view_recorders();
    let view = plan.packet.view_key as usize as *mut u8;
    let scene = plan.scene.cast::<u8>();
    copy(
        plan.saved_rect.as_mut_ptr().cast(),
        view.wrapping_add(0x70).cast(),
        16,
    );
    copy(
        plan.saved_buffer.as_mut_ptr().cast(),
        scene.wrapping_add(0x208).cast(),
        8,
    );
    copy(
        plan.saved_matrices.as_mut_ptr().cast(),
        view.wrapping_add(0xc0).cast(),
        plan.saved_matrices.len(),
    );
    copy(
        plan.saved_parameters.as_mut_ptr().cast(),
        at(plan.cached_parameters),
        plan.saved_parameters.len(),
    );
    unsafe {
        write_unaligned(view.add(0x70).cast::<[i32; 4]>(), plan.packet.output_rect);
        write_unaligned(scene.add(0x208).cast::<[i32; 2]>(), plan.surface_extent);
    }
    // Let the engine rebuild every derived shader value, rather than rewriting a GPU buffer.
    unsafe {
        engine::<unsafe extern "C" fn(*mut c_void)>(0xff_9af0)(view.wrapping_add(0xc0).cast())
    };
    build_postprocess_uniform(plan, view);
    plan.resized = true;
}

/// Puts back the view rectangle, scene buffer size, matrices and cached parameters that
/// `resize_consumers` changed. Callers have already waited for the recorders.
pub(super) fn restore_view_state(plan: &GraphPlan) {
    let view = plan.packet.view_key as usize as *mut u8;
    unsafe {
        write_unaligned(view.add(0x70).cast::<[i32; 4]>(), plan.saved_rect);
        write_unaligned(
            plan.scene.cast::<u8>().add(0x208).cast::<[i32; 2]>(),
            plan.saved_buffer,
        );
        copy_nonoverlapping(
            plan.saved_matrices.as_ptr(),
            view.add(0xc0),
            plan.saved_matrices.len(),
        );
        copy_nonoverlapping(
            plan.saved_parameters.as_ptr(),
            plan.cached_parameters as usize as *mut u8,
            plan.saved_parameters.len(),
        );
    }
}

pub(super) fn restore_view_uniform(plan: &mut GraphPlan) {
    let view = plan.packet.view_key as usize as *mut u8;
    if plan.saved_uniform != 0 {
        move_uniform(
            view.wrapping_add(0x10).cast(),
            (&mut plan.saved_uniform as *mut u64).cast(),
        );
    }
}

unsafe extern "C" fn node_process(node: *mut SrNode, context: *mut c_void) {
    let plan = unsafe { &mut *(*node).plan };
    // A complete native bilinear result exists even if SR refuses on the RHI thread. This uses
    // the engine's own draw/resolve commands and never bypasses later grading or UI processing.
    let fallback: Process = unsafe { original(hook::PROCESS_OUTPUT) };
    unsafe { fallback(node.cast(), context) };
    let owner = unsafe { &*plan.owner };
    if let Some(mut lease) = PassLease::new(owner) {
        let (input, output, exposure, preceding) = unsafe {
            (
                (*node).inputs[0],
                (*node).output.pool,
                (*node).exposure_ref,
                (*node).preceding_exposure_pool,
            )
        };
        lease.input_pool = output_pool(input.node, input.index);
        lease.depth_pool = plan.depth_pool;
        lease.output_pool.store(output, Ordering::SeqCst);
        lease.exposure_pool = if preceding != 0 {
            preceding
        } else {
            output_pool(exposure.node, exposure.index)
        };
        pool_ref(lease.input_pool, 0x28);
        pool_ref(lease.depth_pool, 0x28);
        pool_ref(lease.exposure_pool, 0x28);
        pool_ref(output, 0x28);
        // FPostProcessing receives a pooled render target reference, not an RHI texture.
        lease.motion = pool_texture(plan.velocity, false);
        unsafe { crate::com_resource::retain(lease.motion) };
        let mut list = 0u64;
        read(key(context), 0x28, &mut list);
        let lease = Box::into_raw(lease);
        if !enqueue_leased(owner, at(list), &plan.packet, &pass_lease(lease)) {
            unsafe { release_pass(lease.cast()) };
        }
    }
    resize_consumers(plan);
}

/// Allocation failure while walking the bloom branch, which refuses the plan.
struct Exhausted;

fn push(list: &mut Vec<u64>, value: u64) -> Result<(), Exhausted> {
    list.try_reserve(1).map_err(|_| Exhausted)?;
    list.push(value);
    Ok(())
}

fn bloom_scene_input(bloom: u64, scene: NativeRef) -> Result<*mut NativeRef, Exhausted> {
    let mut pending = Vec::new();
    let mut seen = Vec::new();
    push(&mut pending, bloom)?;
    let mut result: *mut NativeRef = null_mut();
    let downsample = key(engine_address(0xfb_7a00));
    let half_res: [u16; 18] = ascii_wide(b"SceneColorHalfRes\0");
    while seen.len() < 128
        && let Some(node) = pending.pop()
    {
        if node == 0 || seen.contains(&node) {
            continue;
        }
        push(&mut seen, node)?;
        if method(node, 0x70) == downsample {
            let mut name = 0u64;
            let mut label = [0u16; 18];
            if read(node, 0xc8, &mut name)
                && copy(label.as_mut_ptr().cast(), at(name), size_of::<[u16; 18]>())
                && label == half_res
            {
                let input = input_ref(node, 0);
                if input.is_null() {
                    return Ok(null_mut());
                }
                let found = unsafe { *input };
                if found.node != scene.node || found.index != scene.index || !result.is_null() {
                    return Ok(null_mut());
                }
                result = input;
            }
        }
        let mut exhausted = false;
        let bounded = for_each_dependency(node, |reference| {
            if reference.node != 0 && push(&mut pending, reference.node).is_err() {
                exhausted = true;
                return false;
            }
            true
        });
        if exhausted {
            return Err(Exhausted);
        }
        if !bounded {
            return Ok(null_mut());
        }
    }
    Ok(if pending.is_empty() {
        result
    } else {
        null_mut()
    })
}

static ORDERING_REPORTS: AtomicU32 = AtomicU32::new(0);

// Row and column index two matrices and the axes together.
#[allow(clippy::needless_range_loop)]
pub(super) fn make_plan(
    owner: &Renderer,
    context: *mut c_void,
    root: *mut c_void,
    plan: &mut GraphPlan,
) -> bool {
    let Some(config) = owner.config() else {
        return false;
    };
    if config.enabled == 0
        || config.output_width == 0
        || config.output_height == 0
        || config.output_width > 16384
        || config.output_height > 16384
    {
        return false;
    }
    let mut p = scope();
    p.session_id = owner.options.session_id;
    p.role = RSF_AC7_ROLE_SR;
    let (mut frame, mut cached, mut uniform) = (0u32, 0u64, 0u64);
    let context_key = key(context);
    let (render, output) = (&mut p.render_rect, &mut p.output_rect);
    if !read(context_key, 0, &mut p.view_key)
        || p.view_key != POSTPROCESS_VIEW.get()
        || !read(p.view_key, 0, &mut p.family_key)
        || !read(p.family_key, 0x68, &mut frame)
        || !read(p.view_key, 0x70, render)
        || !read(p.view_key, 0x80, output)
        || !read(p.view_key, 0x1418, &mut cached)
        || cached == 0
        || !read(p.view_key, 0x10, &mut uniform)
        || uniform == 0
        || render[0] < 0
        || render[1] < 0
        || render[2] <= render[0]
        || render[3] <= render[1]
        || output[0] < 0
        || output[1] < 0
        || output[2] <= output[0]
        || output[3] <= output[1]
        || output[2] > config.output_width as i32
        || output[3] > config.output_height as i32
    {
        return false;
    }
    if !primary_view(p.view_key, &config) {
        return false;
    }
    let mut bytes = [0u8; RSF_AC7_VIEW_BUFFER_BYTES as usize];
    let mut view = rsf_ac7_view {
        struct_size: size_of::<rsf_ac7_view>() as u32,
        ..Default::default()
    };
    if !copy(bytes.as_mut_ptr().cast(), at(cached), 0xcf0)
        || unsafe {
            rsf_ac7_view_read(
                bytes.as_ptr().cast(),
                bytes.len() as u32,
                RSF_AC7_VIEW_ABI_VERSION,
                &mut view,
            )
        } != VIEW_OK
        || view.has_jitter == 0
        || view.view_width != (p.render_rect[2] - p.render_rect[0]) as u32
        || view.view_height != (p.render_rect[3] - p.render_rect[1]) as u32
    {
        return false;
    }
    let mut cursor = key(root);
    let mut tone = 0u64;
    let mut count = 0;
    while cursor != 0 && count < 64 {
        count += 1;
        if plan.contains(cursor) {
            return false;
        }
        let entry = method(cursor, 0x28);
        // No unreviewed downstream pass is resized.
        let Some(found) = (0..PROCESS_HOOKS).find(|hook| key(site_target(*hook)) == entry) else {
            return false;
        };
        plan.consumers[plan.consumer_count] = cursor;
        plan.consumer_count += 1;
        if hook_role(found) == RSF_AC7_ROLE_TONEMAP {
            tone = cursor;
            break;
        }
        let input = input_ref(cursor, 0);
        if input.is_null() {
            return false;
        }
        cursor = unsafe { (*input).node };
    }
    let input = if tone != 0 {
        input_ref(tone, 0)
    } else {
        null_mut()
    };
    if input.is_null() || unsafe { (*input).node } == 0 {
        return false;
    }
    plan.surface_extent = [config.output_width as i32, config.output_height as i32];
    plan.cached_parameters = cached;
    plan.scene = unsafe { engine::<unsafe extern "C" fn() -> *mut c_void>(0x109_dc70)() };
    plan.owner = owner;
    plan.velocity = POSTPROCESS_VELOCITY.get();
    if !read(key(plan.scene), 0x60, &mut plan.depth_pool)
        || plan.depth_pool == 0
        || plan.velocity == 0
    {
        return false;
    }
    let Some(mut node) = try_box(SrNode::new()) else {
        return false;
    };
    let scene_input = unsafe { *input };
    node.inputs = [scene_input; 2];
    let bloom = input_ref(tone, 1);
    let bloom_input = if !bloom.is_null() && unsafe { (*bloom).node } != 0 {
        match bloom_scene_input(unsafe { (*bloom).node }, scene_input) {
            Ok(found) => found,
            Err(Exhausted) => return false,
        }
    } else {
        null_mut()
    };
    if !bloom_input.is_null() {
        // Current exposure depends on reconstructed colour too. Use already-owned engine
        // exposure preceding this update, avoiding a cycle back through the new SR node.
        let state = unsafe {
            engine::<unsafe extern "C" fn(*mut c_void) -> u64>(0x112_6090)(at(p.view_key))
        };
        let mut index = -1i32;
        if read(state, 0xbd8, &mut index) && (0..=1).contains(&index) {
            read(
                state,
                0xbe0 + index as usize * 8,
                &mut node.preceding_exposure_pool,
            );
        }
    } else {
        let exposure = input_ref(tone, 2);
        if !exposure.is_null() {
            node.exposure_ref = unsafe { *exposure };
        }
    }
    node.extent = [config.output_width as i32, config.output_height as i32];
    node.plan = plan;
    if method(tone, 0x40) == 0 {
        return false;
    }
    // The graph processes dependencies in input order. Finish all original tonemap branches
    // before SR changes view/scene sizes, including auxiliary and additional dependencies.
    let bounded = !bloom_input.is_null()
        || for_each_dependency(tone, |reference| {
            let current = unsafe { *input };
            if reference.node == 0
                || (reference.node == current.node && reference.index == current.index)
            {
                return true;
            }
            if node.auxiliary_count as usize == node.auxiliary.len() {
                return false;
            }
            node.auxiliary[node.auxiliary_count as usize] = reference;
            node.auxiliary_count += 1;
            true
        });
    if !bounded {
        return false;
    }
    // Register through the native graph owner so it retires this heap node through Release.
    let node = Box::into_raw(node);
    type Register = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
    unsafe {
        engine::<Register>(0xe9_3670)(context.cast::<u8>().wrapping_add(0x18).cast(), node.cast())
    };
    unsafe {
        (*input).node = key(node.cast());
        (*input).index = 0;
    }
    if !bloom_input.is_null() {
        unsafe {
            (*bloom_input).node = key(node.cast());
            (*bloom_input).index = 0;
        }
        if ORDERING_REPORTS.fetch_add(1, Ordering::Relaxed) < 6 {
            owner.log(
                c"native SR ordering: SceneColorHalfRes, bloom and exposure now consume reconstructed colour",
            );
        }
    }
    p.pass_key = key(node.cast());
    p.native_frame = u64::from(frame);
    read(p.view_key, 8, &mut p.history_key);
    p.motion_to_uv = [0.5, -0.5];
    p.motion_camera_included = 0;
    p.camera_valid = 1;
    let camera = &mut p.camera;
    camera.struct_size = size_of::<rsf_camera_frame>() as u32;
    camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    camera.render_width = view.view_width;
    camera.render_height = view.view_height;
    camera.output_width = config.output_width;
    camera.output_height = config.output_height;
    camera.near_plane = view.near_plane;
    camera.vertical_fov_radians = view.vertical_fov;
    camera.depth_inverted = 1;
    camera.view_to_clip = view.view_to_clip_no_jitter;
    camera.clip_to_view = view.clip_to_view_no_jitter;
    renderer_identity(owner, &mut p);
    p.flags |= RSF_GAME_RENDER_PRIMARY;
    p.camera.clip_to_previous_clip = view.clip_to_prev_clip;
    p.previous_clip_to_clip = view.prev_clip_to_clip;
    p.camera.jitter_pixels = view.jitter_pixels;
    p.camera.previous_jitter_pixels = view.previous_jitter_pixels;
    let axes = [view.camera_right, view.camera_up, view.camera_forward];
    let camera = &mut p.camera;
    for row in 0..3 {
        for column in 0..3 {
            camera.view_to_world[row * 4 + column] = axes[row][column];
            camera.world_to_view[column * 4 + row] = axes[row][column];
            camera.world_to_view[12 + row] -= view.camera_position[column] * axes[row][column];
        }
    }
    camera.view_to_world[12..15].copy_from_slice(&view.camera_position);
    camera.view_to_world[15] = 1.0;
    camera.world_to_view[15] = 1.0;
    let mut cut = 0u8;
    read(p.view_key, 0xc34, &mut cut);
    if cut != 0 {
        p.flags |= RSF_GAME_RENDER_RESET;
    }
    plan.packet = p;
    true
}

pub(super) unsafe extern "C" fn hooked_context(context: *mut c_void, root: *mut c_void) {
    let _guard = Guard::entry();
    let owner = installed();
    let forward: Process = unsafe { original(hook::CONTEXT) };
    let mut plan = GraphPlan::EMPTY;
    let Some(owner) = owner.filter(|owner| owner.active()) else {
        unsafe { forward(context, root) };
        return;
    };
    if !make_plan(owner, context, root, &mut plan) {
        unsafe { forward(context, root) };
        return;
    }
    let previous = GRAPH_PLAN.replace(&mut plan);
    unsafe { forward(context, root) };
    GRAPH_PLAN.set(previous);
    if plan.resized {
        wait_for_view_recorders();
        restore_view_state(&plan);
        restore_view_uniform(&mut plan);
    }
}

/// The plan of the graph being built on this thread, if it resized `node`'s consumers.
fn planned(node: u64) -> Option<&'static GraphPlan> {
    let plan = GRAPH_PLAN.get();
    unsafe { plan.as_ref() }.filter(|plan| plan.contains(node))
}

pub(super) unsafe extern "C" fn hooked_descriptor<const INDEX: u32>(
    node: *mut c_void,
    out: *mut c_void,
    output: u32,
) -> *mut c_void {
    let _guard = Guard::entry();
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> *mut c_void =
        unsafe { original(hook::DESCRIPTOR_TONEMAP + INDEX) };
    let result = unsafe { forward(node, out, output) };
    if let Some(plan) = planned(key(node)) {
        unsafe {
            write_unaligned(
                result.cast::<u8>().add(0x14).cast::<[i32; 2]>(),
                plan.surface_extent,
            )
        };
    }
    result
}

pub(super) unsafe extern "C" fn hooked_process<const INDEX: u32>(
    node: *mut c_void,
    context: *mut c_void,
) {
    let _guard = Guard::entry();
    let owner = installed();
    let forward: Process = unsafe { original(INDEX) };
    if let Some(owner) = owner {
        owner
            .render_thread
            .store(super::current_thread(), Ordering::SeqCst);
        drain_retired(owner);
    }
    let Some(owner) = owner.filter(|owner| owner.active.load(Ordering::Acquire)) else {
        unsafe { forward(node, context) };
        return;
    };
    let mut pass = scope();
    pass.session_id = owner.options.session_id;
    pass.role = hook_role(INDEX);
    pass.pass_key = key(node);
    // Nimbus front-end composition has its UI in graph input one. The separate
    // HUDCombine path instead reads the view-owned raster. Both are native UI boundaries.
    if INDEX == hook::PROCESS_COMPOSITE {
        pass.role = RSF_GAME_RENDER_UI_COMPOSITE;
    }
    let (mut list, mut frame) = (0u64, 0u32);
    let context_key = key(context);
    let valid = read(context_key, 0, &mut pass.view_key)
        && read(context_key, 0x28, &mut list)
        && read(pass.view_key, 0, &mut pass.family_key)
        && read(pass.family_key, 0x68, &mut frame)
        && read(pass.view_key, 0x70, &mut pass.render_rect)
        && read(pass.view_key, 0x80, &mut pass.output_rect);
    pass.native_frame = u64::from(frame);
    if valid {
        renderer_identity(owner, &mut pass);
        if owner
            .config()
            .is_some_and(|config| primary_view(pass.view_key, &config))
        {
            pass.flags |= RSF_GAME_RENDER_PRIMARY;
        }
    }
    let mut ticket = None;
    let mut leased: *mut PassLease = null_mut();
    if valid && let Some(mut lease) = PassLease::new(owner) {
        let (mut input, mut output) = (0u64, 0u32);
        let getter = method(pass.pass_key, 8);
        if getter != 0 {
            let call: GetPointer = unsafe { super::function(at(getter)) };
            let reference = key(unsafe { call(node, 0) });
            if read(reference, 0, &mut input) && read(reference, 8, &mut output) {
                lease.input_pool = output_pool(input, output);
            }
        }
        pool_ref(lease.input_pool, 0x28);
        if INDEX == hook::PROCESS_COMPOSITE {
            let ui = input_ref(pass.pass_key, 1);
            if !ui.is_null() && unsafe { (*ui).node } != 0 {
                let ui = unsafe { *ui };
                lease.ui_pool = output_pool(ui.node, ui.index);
                pool_ref(lease.ui_pool, 0x28);
            }
        }
        if INDEX == hook::PROCESS_HUD {
            // All reviewed HUD shader variants bind view+0x13a0's render-target resource.
            let (mut target, mut resource, mut texture) = (0u64, 0u64, 0u64);
            if read(pass.view_key, 0x13a0, &mut target)
                && read(target, 0x70, &mut resource)
                && read(resource, 0x30, &mut texture)
            {
                lease.ui = native_texture(texture);
                unsafe { crate::com_resource::retain(lease.ui) };
            }
        }
        let lease = Box::into_raw(lease);
        match open_leased(owner, at(list), &pass, &pass_lease(lease)) {
            Some(opened) => {
                ticket = Some(opened);
                leased = lease;
            }
            None => unsafe { release_pass(lease.cast()) },
        }
    }
    let replaced = INDEX == hook::PROCESS_AA && planned(pass.pass_key).is_some() && {
        let input = input_ref(pass.pass_key, 0);
        let output = node_output(pass.pass_key, 0);
        let pool = if input.is_null() {
            0
        } else {
            let input = unsafe { *input };
            output_pool(input.node, input.index)
        };
        if pool != 0 && !output.is_null() {
            pool_ref(pool, 0x28);
            pool_ref(unsafe { (*output).pool }, 0x30);
            unsafe { (*output).pool = pool };
            true
        } else {
            false
        }
    };
    if !replaced {
        unsafe { forward(node, context) };
    }
    if !leased.is_null() {
        let target = output_pool(pass.pass_key, 0);
        pool_ref(target, 0x28);
        unsafe { (*leased).output_pool.store(target, Ordering::Release) };
    }
    if let Some(ticket) = ticket
        && !close(ticket, at(list))
    {
        owner.deactivate();
        owner.log(c"AC7 renderer deactivated: native command list could not close its pass scope");
    }
}

pub(super) unsafe extern "C" fn hooked_postprocess(
    owner_object: *mut c_void,
    list: *mut c_void,
    view: *mut c_void,
    velocity_ref: *mut c_void,
) {
    let _guard = Guard::entry();
    let owner = installed();
    let (old_view, old_velocity) = (POSTPROCESS_VIEW.get(), POSTPROCESS_VELOCITY.get());
    POSTPROCESS_VIEW.set(key(view));
    let mut velocity = old_velocity;
    read(key(velocity_ref), 0, &mut velocity);
    POSTPROCESS_VELOCITY.set(velocity);
    if let Some(owner) = owner {
        owner
            .render_thread
            .store(super::current_thread(), Ordering::SeqCst);
        drain_retired(owner);
    }
    if let Some(owner) = owner
        && owner.active()
        && velocity != 0
    {
        let mut pass = scope();
        pass.session_id = owner.options.session_id;
        pass.role = RSF_GAME_RENDER_VELOCITY_FINALIZE;
        pass.view_key = key(view);
        pass.pass_key = key(view);
        let (mut frame, mut depth) = (0u32, 0u64);
        let scene = unsafe { engine::<unsafe extern "C" fn() -> *mut c_void>(0x109_dc70)() };
        if owner
            .config()
            .is_some_and(|config| config.enabled != 0 && primary_view(pass.view_key, &config))
            && read(pass.view_key, 0, &mut pass.family_key)
            && read(pass.family_key, 0x68, &mut frame)
            && read(pass.view_key, 8, &mut pass.history_key)
            && read(pass.view_key, 0x70, &mut pass.render_rect)
            && read(pass.view_key, 0x80, &mut pass.output_rect)
            && read(key(scene), 0x60, &mut depth)
            && depth != 0
            && let Some(mut lease) = PassLease::new(owner)
        {
            pass.native_frame = u64::from(frame);
            pass.flags = RSF_GAME_RENDER_PRIMARY;
            lease.depth_pool = depth;
            lease.motion_pool = velocity;
            pool_ref(depth, 0x28);
            pool_ref(velocity, 0x28);
            let lease = Box::into_raw(lease);
            if !enqueue_leased(owner, list, &pass, &pass_lease(lease)) {
                unsafe { release_pass(lease.cast()) };
            }
        }
    }
    let mut final_ticket = None;
    if let Some(owner) = owner
        && owner.active()
    {
        let mut pass = scope();
        pass.session_id = owner.options.session_id;
        pass.role = RSF_GAME_RENDER_FINAL_SCENE;
        pass.view_key = key(view);
        pass.pass_key = pass.view_key;
        let (mut frame, mut target) = (0u32, 0u64);
        if owner
            .config()
            .is_some_and(|config| primary_view(pass.view_key, &config))
            && read(pass.view_key, 0, &mut pass.family_key)
            && read(pass.family_key, 0x68, &mut frame)
            && read(pass.family_key, 0x20, &mut target)
            && read(pass.view_key, 0x70, &mut pass.render_rect)
            && read(pass.view_key, 0x80, &mut pass.output_rect)
        {
            pass.native_frame = u64::from(frame);
            pass.flags = RSF_GAME_RENDER_PRIMARY;
            renderer_identity(owner, &mut pass);
            // The native HUD pass uses this same FRenderTarget virtual slot to access the family
            // output. Query it on the render owner, then retain only its underlying D3D surface.
            let getter = method(target, 8);
            let mut texture = 0u64;
            if getter != 0 {
                let call: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                    unsafe { super::function(at(getter)) };
                let reference = unsafe { call(at(target)) };
                read(key(reference), 0, &mut texture);
            }
            if let Some(mut lease) = PassLease::new(owner) {
                lease.scene_surface = native_texture(texture);
                if !lease.scene_surface.is_null() {
                    unsafe { crate::com_resource::retain(lease.scene_surface) };
                    if pass.source_frame_id != 0 && pass.viewport_key != 0 {
                        let mut tables = owner.tables();
                        let slot =
                            (pass.source_frame_id % tables.final_surfaces.len() as u64) as usize;
                        let surface = &mut tables.final_surfaces[slot];
                        if surface.source == pass.source_frame_id {
                            surface.ambiguous = true;
                        } else {
                            surface.source = pass.source_frame_id;
                            surface.viewport = pass.viewport_key;
                            surface.surface = key(lease.scene_surface);
                            surface.ambiguous = false;
                            owner.final_surface_hint[slot]
                                .store(surface.surface, Ordering::Release);
                        }
                    }
                    let lease = Box::into_raw(lease);
                    final_ticket = open_leased(owner, list, &pass, &pass_lease(lease));
                    if final_ticket.is_none() {
                        unsafe { release_pass(lease.cast()) };
                    }
                }
            }
        }
    }
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { original(hook::POSTPROCESS) };
    unsafe { forward(owner_object, list, view, velocity_ref) };
    if let (Some(ticket), Some(owner)) = (final_ticket, owner)
        && !close(ticket, list)
    {
        owner.deactivate();
        owner.log(c"AC7 final scene source deactivated: native RHI scope could not close");
    }
    POSTPROCESS_VIEW.set(old_view);
    POSTPROCESS_VELOCITY.set(old_velocity);
}
