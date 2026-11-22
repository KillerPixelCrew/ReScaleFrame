// SPDX-License-Identifier: MIT
//! Material and translucency passes: the base pass scope, translucency and its leased scene
//! colour, the output-size UnmodifiedTranslucency UI producer, translucent mesh identity, mesh
//! particle history bindings and the scene colour precision.
use super::graph::{
    GraphPlan, resize_consumers, restore_view_state, restore_view_uniform, retain_view_uniform,
    wait_for_view_recorders,
};
use super::views::for_each_view;
use super::{
    Guard, PassLease, Renderer, SCOPE_READ, at, close, copy, engine, engine_add_ref, hook,
    installed, key, method, move_uniform, open, open_leased, original, pass_lease, pool_ref,
    primary_view, read, release_pass, scope,
};
use crate::sys::rsf_ac7_render_scope;
use crate::sys::{RSF_AC7_VIEW_ABI_VERSION, RSF_AC7_VIEW_BUFFER_BYTES, rsf_ac7_view};
use rsf_ac7::view::rsf_ac7_view_read;
use rsf_game_sys::ffi_util::try_box;
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, RSF_GAME_RENDER_MATERIALS, RSF_GAME_RENDER_PRIMARY,
    RSF_GAME_RENDER_RESET, RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_TRANSLUCENCY_LAYER,
    RSF_GAME_RENDER_TRANSLUCENT_MESH, rsf_camera_frame,
};
use std::cell::{Cell, UnsafeCell};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::{null_mut, write_unaligned};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

type GetScene = unsafe extern "C" fn() -> *mut c_void;

fn scene() -> *mut c_void {
    unsafe { engine::<GetScene>(0x109_dc70)() }
}

static SIZE_REPORTED: AtomicBool = AtomicBool::new(false);

pub(super) unsafe extern "C" fn hooked_translucency_size(scene: *mut c_void, downsample: u8) {
    let _entry = Guard::entry();
    let forward: unsafe extern "C" fn(*mut c_void, u8) =
        unsafe { original(hook::TRANSLUCENCY_SIZE) };
    unsafe { forward(scene, downsample) };
    // Ordinary 3D translucency owns this scale. The UnmodifiedTranslucency UI producer
    // allocates from BufferSize +0x208 under its own output-size view/depth scope.
    if let Some(owner) = installed()
        && owner.active()
        && !SIZE_REPORTED.swap(true, Ordering::Relaxed)
    {
        owner.log(
            c"native ordinary translucency: engine sizing retained; full-resolution UI uses its scoped producer",
        );
    }
}

struct UiProducer {
    plan: GraphPlan,
    depth: u64,
    scaled_depth: u64,
    saved_scaled_uniform: u64,
    scaled_uniform_bound: bool,
    active: bool,
}

thread_local! {
    // Plain data with nothing to drop, so hooks running during thread teardown still reach it.
    static UI_PRODUCER: UnsafeCell<UiProducer> = const {
        UnsafeCell::new(UiProducer {
            plan: GraphPlan::EMPTY,
            depth: 0,
            scaled_depth: 0,
            saved_scaled_uniform: 0,
            scaled_uniform_bound: false,
            active: false,
        })
    };
    pub(super) static TRANSLUCENCY_RENDERER: Cell<*mut c_void> = const { Cell::new(null_mut()) };
}

/// This thread's UI producer state. Engine calls made while it is changed can reenter hooks
/// that read `active`, so it is only reached through this pointer, never a held reference.
fn ui() -> *mut UiProducer {
    UI_PRODUCER.with(UnsafeCell::get)
}

pub(super) fn ui_active() -> bool {
    unsafe { (*ui()).active }
}

pub(super) fn restore_ui_producer() {
    let state = ui();
    if !unsafe { (*state).active } {
        return;
    }
    wait_for_view_recorders();
    let plan = unsafe { &mut (*state).plan };
    let view = plan.packet.view_key as usize as *mut u8;
    unsafe {
        write_unaligned(
            plan.scene.cast::<u8>().add(0x60).cast::<u64>(),
            (*state).depth,
        )
    };
    restore_view_state(plan);
    if unsafe { (*state).scaled_uniform_bound } {
        move_uniform(
            view.wrapping_add(0x18).cast(),
            unsafe { &raw mut (*state).saved_scaled_uniform }.cast(),
        );
        unsafe { (*state).scaled_uniform_bound = false };
    }
    restore_view_uniform(plan);
    unsafe {
        pool_ref((*state).depth, 0x30);
        pool_ref((*state).scaled_depth, 0x30);
        (*state).depth = 0;
        (*state).scaled_depth = 0;
        (*state).active = false;
    }
    plan.resized = false;
}

static UI_REPORTS: AtomicU32 = AtomicU32::new(0);

fn prepare_ui_producer(
    owner: &Renderer,
    scene: *mut c_void,
    list: *mut c_void,
    view: *mut c_void,
) -> bool {
    let state = ui();
    let view_key = key(view);
    let (mut buffer, mut rect) = ([0i32; 2], [0i32; 4]);
    let (mut cached, mut uniform, mut depth) = (0u64, 0u64, 0u64);
    let renderer = TRANSLUCENCY_RENDERER.get();
    let Some(config) = (if unsafe { (*state).active } || renderer.is_null() || !owner.active() {
        None
    } else {
        owner.config()
    }) else {
        return false;
    };
    if config.enabled == 0
        || !primary_view(view_key, &config)
        || !read(view_key, 0x80, &mut rect)
        || !read(view_key, 0x1418, &mut cached)
        || cached == 0
        || !read(view_key, 0x10, &mut uniform)
        || uniform == 0
        || !read(key(scene), 0x60, &mut depth)
        || depth == 0
        || !read(key(scene), 0x208, &mut buffer)
        || buffer[0] <= 0
        || buffer[1] <= 0
        || rect[0] < 0
        || rect[1] < 0
        || rect[2] <= rect[0]
        || rect[3] <= rect[1]
        || rect[2] > config.output_width as i32
        || rect[3] > config.output_height as i32
    {
        return false;
    }
    // Allocate and populate the engine's own enlarged depth before replacing its borrowed
    // scene-depth binding. The depth producer still sees the original scene-sized view.
    let extent = [config.output_width as i32, config.output_height as i32];
    let packed_extent = u64::from(extent[0] as u32) | (u64::from(extent[1] as u32) << 32);
    type Allocate = unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> *mut c_void;
    unsafe { engine::<Allocate>(0x109_e0d0)(scene, list, packed_extent) };
    let mut scaled_depth = 0u64;
    if !read(key(scene), 0x1b8, &mut scaled_depth) || scaled_depth == 0 {
        return false;
    }
    let horizontal = config.output_width as f32 / buffer[0] as f32;
    let vertical = config.output_height as f32 / buffer[1] as f32;
    let scale = if horizontal < vertical {
        vertical
    } else {
        horizontal
    };
    if !scale.is_finite() || !(1.0..=4.0).contains(&scale) {
        return false;
    }
    type Depth = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, f32, u8);
    unsafe {
        engine::<Depth>(0xee_93f0)(
            renderer,
            list,
            at(scaled_depth.wrapping_add(8)),
            view,
            scale,
            0,
        )
    };
    let plan = unsafe { &mut (*state).plan };
    plan.owner = owner;
    plan.scene = scene;
    plan.packet.view_key = view_key;
    plan.cached_parameters = cached;
    plan.packet.output_rect = rect;
    plan.surface_extent = extent;
    unsafe {
        (*state).depth = depth;
        (*state).scaled_depth = scaled_depth;
    }
    pool_ref(depth, 0x28);
    pool_ref(scaled_depth, 0x28);
    resize_consumers(plan);
    if plan.saved_uniform == 0 {
        // Restoration must remain possible even if native uniform creation refused.
        unsafe { (*state).active = true };
        restore_ui_producer();
        return false;
    }
    // Patched translucent material policies can select the scaled-view slot even for this
    // game-specific UI pass. It may be empty when no ordinary translucency pass preceded it.
    // Both selectors must receive this pass's complete output-size unjittered view.
    let mut current_uniform = 0u64;
    if !read(view_key, 0x10, &mut current_uniform) || current_uniform == 0 {
        unsafe { (*state).active = true };
        restore_ui_producer();
        return false;
    }
    unsafe { (*state).saved_scaled_uniform = retain_view_uniform(view.cast(), 0x18) };
    engine_add_ref(current_uniform);
    move_uniform(
        view.cast::<u8>().wrapping_add(0x18).cast(),
        (&mut current_uniform as *mut u64).cast(),
    );
    unsafe {
        (*state).scaled_uniform_bound = true;
        write_unaligned(scene.cast::<u8>().add(0x60).cast::<u64>(), scaled_depth);
        (*state).active = true;
    }
    if UI_REPORTS.fetch_add(1, Ordering::Relaxed) < 6 {
        owner.log(
            c"native UnmodifiedTranslucency: output-size allocation, depth and unjittered view before rasterization",
        );
    }
    true
}

pub(super) unsafe extern "C" fn hooked_unmodified_begin(
    scene: *mut c_void,
    list: *mut c_void,
    view: *mut c_void,
    clear: u8,
    glow: u8,
    extra: u8,
) -> u64 {
    let _entry = Guard::entry();
    let prepared = installed().is_some_and(|owner| prepare_ui_producer(owner, scene, list, view));
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u8, u8, u8) -> u64 =
        unsafe { original(hook::UNMODIFIED_BEGIN) };
    let result = unsafe { forward(scene, list, view, clear, glow, extra) };
    if prepared && result & 0xff == 0 {
        restore_ui_producer();
    }
    result
}

pub(super) unsafe extern "C" fn hooked_unmodified_resolve(
    scene: *mut c_void,
    list: *mut c_void,
    view: *mut c_void,
    glow: u8,
    extra: u8,
) {
    let _entry = Guard::entry();
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u8, u8) =
        unsafe { original(hook::UNMODIFIED_RESOLVE) };
    unsafe { forward(scene, list, view, glow, extra) };
    let state = ui();
    if unsafe {
        (*state).active
            && (*state).plan.scene == scene
            && (*state).plan.packet.view_key == key(view)
    } {
        restore_ui_producer();
    }
}

/// FSceneRenderTargets::GetSceneColor: the shading path at +0x25c and the alpha/format selection
/// at +0x260/+0x230 pick SceneColor[Mobile/HighEnd/HighEndWithAlpha] at +0x30/+0x38/+0x40,
/// matching AC7_FSceneRenderTargets_AllocateSceneColor (RVA 0x1095010). Unknown paths refuse.
fn scene_colour_pool(scene: *mut c_void) -> u64 {
    let (mut path, mut format, mut alpha, mut pool) = (0i32, 0i32, 0u8, 0u64);
    let targets = key(scene);
    if !read(targets, 0x25c, &mut path)
        || !read(targets, 0x260, &mut alpha)
        || !read(targets, 0x230, &mut format)
    {
        return 0;
    }
    let offset = match path {
        0 => 0x30,
        1 if alpha != 0 || format == 10 => 0x40,
        1 => 0x38,
        _ => 0,
    };
    if offset != 0 && read(targets, offset, &mut pool) {
        pool
    } else {
        0
    }
}

/// Identity of the single primary view of a renderer that SR reconstructs, or false.
fn primary_scope(
    owner: &Renderer,
    renderer: *mut c_void,
    role: u32,
    out: &mut rsf_ac7_render_scope,
) -> bool {
    if !owner.active() {
        return false;
    }
    let Some(config) = owner.config().filter(|config| config.enabled != 0) else {
        return false;
    };
    *out = scope();
    out.role = role;
    out.session_id = owner.options.session_id;
    {
        super::rsf_ac7_scope_primary_facts(
            SCOPE_READ,
            key(renderer),
            config.output_width,
            config.output_height,
            Some(out),
        )
    }
}

/// Passes 0 (standard), 1 (after DOF, the separate layer at scene+0x1b0, allocator RVA 0x109f4d0)
/// and 2 (all) of the single primary view. Pass 4 is AC7's interface layer and stays outside.
/// Scene colour is leased through the pass so the runtime can snapshot it at begin and compare
/// at end.
fn open_translucency(
    owner: &Renderer,
    renderer: *mut c_void,
    list: *mut c_void,
    pass: u32,
    scene: *mut c_void,
) -> Option<(*mut c_void, *mut PassLease)> {
    let mut translucency = scope();
    if pass > 2
        || scene.is_null()
        || !primary_scope(
            owner,
            renderer,
            RSF_GAME_RENDER_TRANSLUCENCY,
            &mut translucency,
        )
    {
        return None;
    }
    translucency.pass_key = u64::from(pass);
    translucency.flags = RSF_GAME_RENDER_PRIMARY
        | if pass == 1 {
            RSF_GAME_RENDER_TRANSLUCENCY_LAYER
        } else {
            0
        };
    let colour = scene_colour_pool(scene);
    if colour == 0 {
        return None;
    }
    let mut lease = PassLease::new(owner)?;
    lease.input_pool = colour;
    pool_ref(colour, 0x28);
    let lease = Box::into_raw(lease);
    match open_leased(owner, list, &translucency, &pass_lease(lease)) {
        Some(ticket) => Some((ticket, lease)),
        None => {
            pool_ref(colour, 0x30);
            drop(unsafe { Box::from_raw(lease) });
            None
        }
    }
}

/// RenderBasePass, RVA 0xebf050 (DeferredShadingRenderer.cpp:954): the material draws of the
/// primary view, including parallel lists submitted before it returns, inside one queued scope.
pub(super) unsafe extern "C" fn hooked_base_pass(
    renderer: *mut c_void,
    list: *mut c_void,
    access: u32,
) -> u8 {
    let _lifetime = Guard::outer();
    let owner = installed();
    let mut materials = scope();
    let ticket = owner.and_then(|owner| {
        if primary_scope(owner, renderer, RSF_GAME_RENDER_MATERIALS, &mut materials) {
            open(owner, list, &materials)
        } else {
            None
        }
    });
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) -> u8 =
        unsafe { original(hook::BASE_PASS) };
    let result = unsafe { forward(renderer, list, access) };
    if let (Some(ticket), Some(owner)) = (ticket, owner)
        && !close(ticket, list)
    {
        owner.log(c"AC7 base pass scope could not close");
    }
    result
}

static SCALED_REPORTS: AtomicU32 = AtomicU32::new(0);
static LEASE_REPORTS: AtomicU32 = AtomicU32::new(0);

pub(super) unsafe extern "C" fn hooked_translucency_render(
    renderer: *mut c_void,
    list: *mut c_void,
    pass: u32,
) {
    let _lifetime = Guard::outer();
    let previous = TRANSLUCENCY_RENDERER.replace(renderer);
    let owner = installed();
    let scene = scene();
    let mut layer_scale = 1.0f32;
    read(key(scene), 0x220, &mut layer_scale);
    let mut refused = false;
    if let Some(owner) = owner
        && owner.active()
        && owner.config().is_some_and(|config| config.enabled != 0)
        && layer_scale != 1.0
    {
        for_each_view(renderer, |view| {
            let (mut scaled_uniform, mut parameters, mut normal_uniform) = (0u64, 0u64, 0u64);
            if !read(view, 0x1418, &mut parameters)
                || parameters == 0
                || !read(view, 0x10, &mut normal_uniform)
                || normal_uniform == 0
            {
                return true;
            }
            if read(view, 0x18, &mut scaled_uniform) && scaled_uniform != 0 {
                return true;
            }
            // The stock producer is conditional on the ordinary translucency pass. AC7's
            // unmodified/extra passes can select this slot without taking that branch first.
            type Producer = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);
            unsafe { engine::<Producer>(0x116_ea00)(renderer, list, at(view)) };
            if !read(view, 0x18, &mut scaled_uniform) || scaled_uniform == 0 {
                owner.log(
                    c"AC7 translucency pass refused: native scaled-view uniform producer returned no buffer",
                );
                refused = true;
                return false;
            }
            if SCALED_REPORTS.fetch_add(1, Ordering::Relaxed) < 6 {
                owner.log(c"native translucency: populated missing scaled-view uniform before material selection");
            }
            true
        });
    }
    if refused {
        TRANSLUCENCY_RENDERER.set(previous);
        return;
    }
    let opened = owner.and_then(|owner| open_translucency(owner, renderer, list, pass, scene));
    let forward: unsafe extern "C" fn(*mut c_void, *mut c_void, u32) =
        unsafe { original(hook::TRANSLUCENCY_RENDER) };
    unsafe { forward(renderer, list, pass) };
    if let (Some((ticket, lease)), Some(owner)) = (opened, owner) {
        // The layer is allocated on demand inside the pass. Publish it before end is queued.
        let mut layer = 0u64;
        if pass == 1 && read(key(scene), 0x1b0, &mut layer) && layer != 0 {
            pool_ref(layer, 0x28);
            unsafe { (*lease).output_pool.store(layer, Ordering::Release) };
        }
        if !close(ticket, list) {
            owner.log(c"AC7 translucency scope could not close");
        }
        if LEASE_REPORTS.fetch_add(1, Ordering::Relaxed) < 3 {
            owner.log(
                c"native translucency: scene colour leased around passes 0/1/2 for reactive and transparency inputs",
            );
        }
    }
    // Stereo/empty branches may omit Resolve. Do not leave a changed view after this producer.
    if ui_active() {
        restore_ui_producer();
    }
    TRANSLUCENCY_RENDERER.set(previous);
}

/// DrawMesh is shared by static and dynamic translucency. Its material, vertex factory and
/// primitive identity must travel with the queued draws, including parallel command recording.
type TranslucencyMesh = unsafe extern "C" fn(
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *mut c_void,
    *const u64,
    *mut c_void,
    u8,
    *mut c_void,
    u32,
) -> u8;

#[allow(clippy::too_many_arguments)]
pub(super) unsafe extern "C" fn hooked_translucency_mesh(
    list: *mut c_void,
    view_object: *mut c_void,
    context: *mut c_void,
    mesh: *mut c_void,
    elements: *const u64,
    state: *mut c_void,
    pre_fog: u8,
    proxy: *mut c_void,
    hit: u32,
) -> u8 {
    let _lifetime = Guard::outer();
    let forward: TranslucencyMesh = unsafe { original(hook::TRANSLUCENCY_MESH) };
    let owner = installed();
    let mut mesh_scope = scope();
    let view = key(view_object);
    let (mut material_proxy, mut material, mut cached) = (0u64, 0u64, 0u64);
    let (mut feature, mut frame, mut pass, mut cut) = (0u32, 0u32, u32::MAX, 0u8);
    let mut ticket = None;
    if let Some(owner) = owner
        && owner.active()
        && let Some(config) = owner.config()
        && config.enabled != 0
        && !proxy.is_null()
        && !elements.is_null()
        && read(key(context), 0, &mut pass)
        && pass <= 2
        && primary_view(view, &config)
        && read(view, 0, &mut mesh_scope.family_key)
        && read(mesh_scope.family_key, 0x68, &mut frame)
        && read(view, 8, &mut mesh_scope.history_key)
        && read(view, 0x70, &mut mesh_scope.render_rect)
        && read(view, 0x80, &mut mesh_scope.output_rect)
        && read(view, 0xc34, &mut cut)
        && read(view, 0x13e8, &mut feature)
        && read(view, 0x1418, &mut cached)
        && cached != 0
        && read(key(mesh), 0x90, &mut mesh_scope.vertex_factory_key)
        && read(key(mesh), 0x98, &mut material_proxy)
        && material_proxy != 0
        && copy(
            (&mut mesh_scope.mesh_element_mask as *mut u64).cast(),
            elements.cast(),
            size_of::<u64>(),
        )
    {
        let get_material = method(material_proxy, 0x40);
        if get_material != 0 {
            let call: unsafe extern "C" fn(*mut c_void, u32) -> u64 =
                unsafe { super::function(at(get_material)) };
            material = unsafe { call(at(material_proxy), feature) };
        }
        let (blend, clip) = (method(material, 0x198), method(material, 0x1b8));
        if blend != 0 && clip != 0 {
            let blend: unsafe extern "C" fn(*mut c_void) -> u32 =
                unsafe { super::function(at(blend)) };
            let clip: unsafe extern "C" fn(*mut c_void) -> f32 =
                unsafe { super::function(at(clip)) };
            mesh_scope.material_blend = unsafe { blend(at(material)) };
            mesh_scope.material_opacity_clip = unsafe { clip(at(material)) };
            let mut bytes = [0u8; RSF_AC7_VIEW_BUFFER_BYTES as usize];
            let mut camera = rsf_ac7_view {
                struct_size: size_of::<rsf_ac7_view>() as u32,
                ..Default::default()
            };
            let opacity = mesh_scope.material_opacity_clip;
            if (2..=5).contains(&mesh_scope.material_blend)
                && opacity.is_finite()
                && (0.0..=1.0).contains(&opacity)
                && copy(bytes.as_mut_ptr().cast(), at(cached), 0xcf0)
                && unsafe {
                    rsf_ac7_view_read(
                        bytes.as_ptr().cast(),
                        bytes.len() as u32,
                        RSF_AC7_VIEW_ABI_VERSION,
                        &mut camera,
                    )
                } == 0
                && camera.has_jitter != 0
            {
                mesh_scope.role = RSF_GAME_RENDER_TRANSLUCENT_MESH;
                mesh_scope.session_id = owner.options.session_id;
                mesh_scope.view_key = view;
                mesh_scope.native_frame = u64::from(frame);
                mesh_scope.pass_key = key(mesh);
                mesh_scope.primitive_key = key(proxy);
                mesh_scope.material_key = material_proxy;
                // FMeshBatch's inline element array is at +0, external storage +0x60, Num +0x68.
                // AC7 element stride 0x60: NumPrimitives +0x34, NumInstances +0x38. Match the
                // native DrawMesh element loop; copy a bound, never a CPU pointer, into RHI work.
                let (mut storage, mut count, mut capacity) = (0u64, 0i32, 0u64);
                let address = key(mesh);
                if read(address, 0x60, &mut storage)
                    && read(address, 0x68, &mut count)
                    && (1..=64).contains(&count)
                {
                    if storage == 0 {
                        storage = address;
                    }
                    for index in 0..count as u64 {
                        if mesh_scope.mesh_element_mask & (1u64 << index) == 0 {
                            continue;
                        }
                        let (mut primitives, mut instances) = (0u32, 0u32);
                        let element = storage.wrapping_add(index * 0x60);
                        if !read(element, 0x34, &mut primitives)
                            || !read(element, 0x38, &mut instances)
                        {
                            capacity = 0;
                            break;
                        }
                        let vertices = u64::from(primitives) * 3;
                        if vertices != 0 && u64::from(instances) > u64::from(u32::MAX) / vertices {
                            capacity = 0;
                            break;
                        }
                        capacity += vertices * u64::from(instances);
                        if capacity > u64::from(u32::MAX) {
                            capacity = 0;
                            break;
                        }
                    }
                }
                mesh_scope.geometry_capacity = capacity as u32;
                mesh_scope.flags =
                    RSF_GAME_RENDER_PRIMARY | if cut != 0 { RSF_GAME_RENDER_RESET } else { 0 };
                mesh_scope.camera.struct_size = size_of::<rsf_camera_frame>() as u32;
                mesh_scope.camera.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
                mesh_scope.camera.render_width = camera.view_width;
                mesh_scope.camera.render_height = camera.view_height;
                mesh_scope.camera.jitter_pixels = camera.jitter_pixels;
                mesh_scope.camera.previous_jitter_pixels = camera.previous_jitter_pixels;
                mesh_scope.camera.clip_to_previous_clip = camera.clip_to_prev_clip;
                mesh_scope.camera_valid = 1;
                ticket = open(owner, list, &mesh_scope);
            }
        }
    }
    let result = unsafe {
        forward(
            list,
            view_object,
            context,
            mesh,
            elements,
            state,
            pre_fog,
            proxy,
            hit,
        )
    };
    if let (Some(ticket), Some(owner)) = (ticket, owner)
        && !close(ticket, list)
    {
        owner.log(c"AC7 translucent mesh scope could not close");
    }
    result
}

/// An engine RHI command (FRHICommandBase layout: next, execute) that binds the native previous
/// mesh-particle transforms on the RHI context.
#[repr(C)]
pub(super) struct ParticleBindingCommand {
    pub(super) next: *mut ParticleBindingCommand,
    pub(super) execute: unsafe extern "C" fn(*mut c_void, *mut ParticleBindingCommand),
    owner: *const Renderer,
    references: *mut PassLease,
    shader: u64,
    previous: u64,
}

pub(super) unsafe extern "C" fn execute_particle_binding(
    list: *mut c_void,
    command: *mut ParticleBindingCommand,
) {
    let _guard = Guard::entry();
    let mut context = 0u64;
    let command = unsafe { Box::from_raw(command) };
    if read(key(list), 0x20, &mut context) {
        let set = method(context, 0x1d0);
        if set != 0 {
            let set: unsafe extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void) =
                unsafe { super::function(at(set)) };
            unsafe { set(at(context), at(command.shader), 127, at(command.previous)) };
        }
    }
    unsafe { release_pass(command.references.cast()) };
    unsafe {
        (*command.owner)
            .pending_particle_bindings
            .fetch_sub(1, Ordering::SeqCst)
    };
}

/// FMeshParticleVertexFactoryShaderParametersVS::SetMesh, matched through the parameter binder
/// vtable and PrevTransformBuffer string. The native factory owns the sorted instance history.
pub(super) unsafe extern "C" fn hooked_mesh_particle_parameters(
    parameters: *mut c_void,
    list: *mut c_void,
    shader: *mut c_void,
    factory: *mut c_void,
    view: *mut c_void,
    element: *mut c_void,
) {
    let _lifetime = Guard::outer();
    type SetMesh = unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        *mut c_void,
    );
    let forward: SetMesh = unsafe { original(hook::MESH_PARTICLE_PARAMETERS) };
    unsafe { forward(parameters, list, shader, factory, view, element) };
    let Some(owner) = installed().filter(|owner| owner.active() && !owner.quiescing()) else {
        return;
    };
    let (mut resource, mut vertex, mut previous) = (0u64, 0u64, 0u64);
    if !read(key(shader), 0x48, &mut resource)
        || !read(resource, 0x38, &mut vertex)
        || vertex == 0
        || !read(key(factory), 0x4a0, &mut previous)
    {
        return;
    }
    let command = try_box(ParticleBindingCommand {
        next: null_mut(),
        execute: execute_particle_binding,
        owner,
        references: null_mut(),
        shader: vertex,
        previous,
    });
    let references = PassLease::new(owner);
    let (Some(command), Some(mut references)) = (command, references) else {
        return;
    };
    if references.uniforms.try_reserve(2).is_err() {
        return;
    }
    references.uniforms.push(vertex);
    if previous != 0 {
        references.uniforms.push(previous);
    }
    for pointer in &references.uniforms {
        engine_add_ref(*pointer);
    }
    let references = Box::into_raw(references);
    let command = Box::into_raw(command);
    unsafe { (*command).references = references };
    let (mut tail, mut count, mut executing) = (0u64, 0u32, 0u8);
    let address = key(list);
    let mut existing = 1u64;
    if read(address, 8, &mut tail)
        && tail != 0
        && read(tail, 0, &mut existing)
        && existing == 0
        && read(address, 0x10, &mut executing)
        && executing == 0
        && read(address, 0x14, &mut count)
        && count != u32::MAX
    {
        owner
            .pending_particle_bindings
            .fetch_add(1, Ordering::SeqCst);
        unsafe {
            *(tail as usize as *mut *mut ParticleBindingCommand) = command;
            *((address + 8) as usize as *mut *mut *mut ParticleBindingCommand) =
                &raw mut (*command).next;
            *((address + 0x14) as usize as *mut u32) = count + 1;
        }
    } else {
        unsafe { release_pass(references.cast()) };
        drop(unsafe { Box::from_raw(command) });
    }
}

static PRECISION_REPORTED: AtomicBool = AtomicBool::new(false);

pub(super) unsafe extern "C" fn hooked_scene_colour_format(scene: *mut c_void) -> u32 {
    let _entry = Guard::entry();
    let forward: unsafe extern "C" fn(*mut c_void) -> u32 =
        unsafe { original(hook::SCENE_COLOUR_FORMAT) };
    let mut feature = 0i32;
    let Some(owner) = installed().filter(|owner| owner.active()) else {
        return unsafe { forward(scene) };
    };
    if !read(key(scene), 0x258, &mut feature) || feature < 2 {
        return unsafe { forward(scene) };
    }
    // Preserve precision where lighting and sky first write colour. Widening an R11 texture
    // after rendering cannot recover its discarded mantissa bits.
    if !PRECISION_REPORTED.swap(true, Ordering::SeqCst) {
        owner.log(c"native scene colour: PF_FloatRGBA selected at allocation and shader-parameter producer, RVA 0x109f300");
    }
    10
}
