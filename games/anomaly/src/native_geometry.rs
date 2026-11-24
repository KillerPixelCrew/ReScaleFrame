// SPDX-License-Identifier: MIT
//! Owns geometry admission, scoped engine facts and the draw-time snapshot.
//! The native detours bracket calls with stack storage so no engine trampoline
//! or C++ unwind crosses Rust. Engine reads have separate guarded native leaves.

pub(crate) mod coverage;
#[cfg(all(windows, feature = "sr-fixtures"))]
mod fixture;
mod hook_sites;
mod profile;
mod scopes;
#[cfg(test)]
mod tests;
mod vertex_constants;
// The dx10ConstantBuffer layout the SR position correction also reads.
pub(crate) use vertex_constants::{
    DATA_OFFSET as CONSTANT_BUFFER_DATA, SIZE_OFFSET as CONSTANT_BUFFER_SIZE,
    SLOTS as CONSTANT_BUFFER_SLOTS,
};

use crate::engine_guards::read_bytes;
#[cfg_attr(not(windows), allow(unused_imports))]
use crate::engine_guards::{Image, Read};
use crate::geometry_registry::{
    IdentityRegistry, Registry, classify, find, probe_visual, register, retire, take_live,
};
use crate::motion::from_engine_matrix;
use crate::native_geometry_abi::*;
use crate::sys::{rsf_anomaly_geometry_draw as Draw, rsf_anomaly_geometry_key as Key, *};
use coverage::COUNTERS;
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::ptr::{self, with_exposed_provenance};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

const UNKNOWN_PASS: u8 = RSF_ANOMALY_PASS_UNKNOWN as u8;
const MAX_HIERARCHY: usize = 4096;
const MAX_CAPTURE_BONES: u32 = 1024;
const BONE_BATCH: usize = 64;
const SKELETON_PARENT_OFFSET: usize = 8;
const SKELETON_MODE_OFFSET: usize = 56;
const SKELETON_COUNT_OFFSET: usize = 60;
const BONE_INSTANCE_STRIDE: usize = 168;
const BONE_RENDER_TRANSFORM_OFFSET: usize = 64;
const BONE_INSTANCES_OFFSET: usize = 208; // CKinematics+128 IKinematics +0x50.
const TREE_PM_LAST_LOD_OFFSET: usize = 0x108;
const HIERARCHY_CHILDREN_OFFSET: usize = 96; // FHierrarhyVisual child vector, then BOOL bDontDelete.
const SBONES_PALETTE_LAYOUT: u64 = 0x53424f4e45530001; // Three float4 columns per bone.
const ENGINE_IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

static REGISTRY: IdentityRegistry = Mutex::new(Registry::new());
static CONTROL: Mutex<()> = Mutex::new(());
static MODELS: Mutex<()> = Mutex::new(());
static ENGINE: RwLock<Engine> = RwLock::new(Engine {
    module: 0,
    build: 0,
});
static HOOKS: AtomicBool = AtomicBool::new(false);
static CAPTURE: AtomicBool = AtomicBool::new(false);
static FOLIAGE: AtomicBool = AtomicBool::new(false);
static RETIRED: RwLock<(usize, Option<Retired>)> = RwLock::new((0, None));
static DRAW: RwLock<(usize, Option<DrawSink>)> = RwLock::new((0, None));
static FLUSH: RwLock<Option<Flush>> = RwLock::new(None);
static NEAR: RwLock<Option<Near>> = RwLock::new(None);

fn capturing() -> bool {
    CAPTURE.load(Ordering::Acquire)
}

fn fault() {
    COUNTERS.capture_faults.increment();
    CAPTURE.store(false, Ordering::Release);
}

fn rsf_anomaly_geometry_binding() -> Engine {
    *ENGINE.read().unwrap()
}

/// Admits the selected build before any detour is written. The image size
/// comes first, then every layout guard behind the native reader, then every
/// detour entry. Each site also needs its unique prefix where one is profiled.
fn guard(platform: &Platform, engine: Engine, size: u32) -> bool {
    if engine.module == 0 {
        return false;
    }
    let (Some(geometry), Some(image_size)) =
        (profile::geometry(engine), profile::image_size(engine))
    else {
        return false;
    };
    if size != image_size {
        return false;
    }
    let image = Image::new(
        with_exposed_provenance(engine.module),
        size,
        platform.read.unwrap(),
    );
    // The profile tables are static and their unique guards are NUL-terminated.
    unsafe {
        image.matches_sites(geometry.layout_sites, geometry.unique)
            && image.matches_sites(geometry.sites, geometry.unique)
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_install(platform: &Platform, engine: Engine, size: u32) -> bool {
    let _control = CONTROL.lock().unwrap();
    if HOOKS.load(Ordering::Acquire) || !guard(platform, engine, size) {
        return false;
    }
    let created = unsafe { (platform.created.unwrap())() };
    // Trampolines are pinned to the original exact executable and layout.
    if created && rsf_anomaly_geometry_binding() != engine {
        return false;
    }
    {
        let _models = MODELS.lock().unwrap();
        reset_registry();
        *ENGINE.write().unwrap() = engine;
        CAPTURE.store(true, Ordering::Release);
    }
    let installed = unsafe {
        if created {
            (platform.enable.unwrap())()
        } else {
            (platform.install.unwrap())(engine, size)
        }
    };
    HOOKS.store(installed, Ordering::Release);
    if !installed {
        CAPTURE.store(false, Ordering::Release);
        rsf_anomaly_geometry_reset();
    }
    installed
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_disable(platform: &Platform) -> bool {
    let _control = CONTROL.lock().unwrap();
    CAPTURE.store(false, Ordering::Release);
    if !unsafe { (platform.created.unwrap())() } {
        return false;
    }
    let disabled = unsafe { (platform.disable.unwrap())() };
    if disabled {
        HOOKS.store(false, Ordering::Release);
    }
    rsf_anomaly_geometry_reset();
    disabled
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_geometry_active() -> bool {
    HOOKS.load(Ordering::Acquire) && capturing()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_test_bind(engine: Engine) {
    *ENGINE.write().unwrap() = engine;
    // The fixture's own detours stand in for the installed hook set.
    HOOKS.store(true, Ordering::Release);
    CAPTURE.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_retired(user: usize, callback: Option<Retired>) {
    let _models = MODELS.lock().unwrap();
    *RETIRED.write().unwrap() = (user, callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_draw(user: usize, callback: Option<DrawSink>) {
    *DRAW.write().unwrap() = (user, callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_flush(callback: Option<Flush>) {
    *FLUSH.write().unwrap() = callback;
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_geometry_set_near(callback: Option<Near>) {
    *NEAR.write().unwrap() = callback;
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_publish_flush(facts: *const NativeDrawFacts) {
    let callback = *FLUSH.read().unwrap();
    if let Some(callback) = callback {
        // The native flush supplies a live stack record for this call.
        unsafe { callback(facts) };
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_after_near(render: *mut c_void) {
    scopes::near_enter();
    let callback = *NEAR.read().unwrap();
    if let Some(callback) = callback {
        unsafe { callback(render) };
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn rsf_anomaly_geometry_set_foliage(enabled: bool) {
    FOLIAGE.store(enabled, Ordering::Relaxed);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_foliage() -> bool {
    FOLIAGE.load(Ordering::Relaxed)
}

fn report_retired(key: &Key) {
    COUNTERS.models_retired.increment();
    #[cfg(windows)]
    crate::motion::record_retirement(*key);
    let (user, callback) = *RETIRED.read().unwrap();
    if let Some(callback) = callback {
        unsafe { callback(ptr::with_exposed_provenance_mut(user), key) };
    }
}

fn reset_registry() {
    for key in take_live(&REGISTRY) {
        report_retired(&key);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_reset() {
    let _models = MODELS.lock().unwrap();
    reset_registry();
}

fn read_pointer(platform: &Platform, address: usize) -> Option<usize> {
    read_bytes(platform.read.unwrap(), address).map(usize::from_ne_bytes)
}

fn read_children(
    platform: &Platform,
    vtable_rvas: &[u32; 4],
    module: usize,
    visual: usize,
) -> Option<ChildRange> {
    let vtable = read_pointer(platform, visual)?;
    let mut range = ChildRange::default();
    if !vtable_rvas
        .iter()
        .any(|rva| module.checked_add(*rva as usize) == Some(vtable))
    {
        return Some(range);
    }
    let address = visual.checked_add(HIERARCHY_CHILDREN_OFFSET)?;
    let size = size_of::<ChildRange>();
    let borrowed = i32::from_ne_bytes(read_bytes(
        platform.read.unwrap(),
        address.checked_add(size)?,
    )?);
    if borrowed != 0 {
        return Some(range);
    }
    // The native vector is three pointer-sized addresses. The profiled layout is asserted
    // by the geometry evidence, and the copy is guarded even when an engine object has gone away.
    unsafe {
        (platform.read.unwrap())(
            ptr::from_mut(&mut range).cast(),
            with_exposed_provenance(address),
            size,
        )
    }
    .then_some(range)
}

fn visual_tree(platform: &Platform, root: usize) -> Option<Vec<usize>> {
    let mut tree = Vec::new();
    if root == 0 {
        return Some(tree);
    }
    tree.try_reserve(1).ok()?;
    tree.push(root);
    let engine = rsf_anomaly_geometry_binding();
    if engine.module == 0 {
        return Some(tree);
    }
    let vtables = profile::geometry(engine)?.hierarchy_vtables;
    let mut index = 0;
    while let Some(&visual) = tree.get(index) {
        index += 1;
        let range = read_children(platform, &vtables, engine.module, visual)?;
        let bytes = range.end.checked_sub(range.first)?;
        let pointer_size = size_of::<usize>();
        if range.capacity < range.end
            || !bytes.is_multiple_of(pointer_size)
            || bytes / pointer_size > MAX_HIERARCHY
        {
            return None;
        }
        for item in (range.first..range.end).step_by(pointer_size) {
            let child = read_pointer(platform, item)?;
            if child == 0 {
                return None;
            }
            if !tree.contains(&child) {
                if tree.len() == MAX_HIERARCHY {
                    return None;
                }
                tree.try_reserve(1).ok()?;
                tree.push(child);
            }
        }
    }
    Some(tree)
}

fn retire_tree(owner: u64, tree: &[usize]) {
    for &visual in tree {
        let visual = visual as u64;
        let generation = retire(&REGISTRY, owner, visual);
        if generation != 0 {
            report_retired(&Key {
                owner,
                visual,
                generation,
            });
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_factory_complete(
    platform: &Platform,
    owner: usize,
    visual: usize,
) {
    scopes::leave_factory();
    if !capturing() || visual == 0 || scopes::in_factory() {
        return;
    }
    let _models = MODELS.lock().unwrap();
    if !capturing() {
        return;
    }
    let Some(tree) = visual_tree(platform, visual) else {
        fault();
        return;
    };
    retire_tree(owner as u64, &tree);
    for visual in tree {
        if register(&REGISTRY, owner as u64, visual as u64) == 0 {
            fault();
            break;
        }
        COUNTERS.models_registered.increment();
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_model_delete(
    platform: &Platform,
    owner: usize,
    reference: usize,
) {
    if !capturing() {
        return;
    }
    let _models = MODELS.lock().unwrap();
    if !capturing() {
        return;
    }
    if let Some(tree) =
        read_pointer(platform, reference).and_then(|root| visual_tree(platform, root))
    {
        retire_tree(owner as u64, &tree);
    } else {
        fault();
    }
}

fn read_pass(platform: &Platform, engine: Engine) -> u8 {
    let phase = engine_layout(engine)
        .and_then(|layout| engine.module.checked_add(layout.phase as usize))
        .and_then(|address| read_bytes(platform.read.unwrap(), address))
        .map(u32::from_ne_bytes);
    match phase {
        Some(0) => RSF_ANOMALY_PASS_MAIN as u8,
        Some(1) => RSF_ANOMALY_PASS_SHADOW as u8,
        _ => UNKNOWN_PASS,
    }
}

fn engine_layout(engine: Engine) -> Option<Layout> {
    if engine.module == 0 {
        return None;
    }
    profile::layout(engine)
}

fn world_address(engine: Engine) -> Option<usize> {
    engine
        .module
        .checked_add(engine_layout(engine)?.world as usize)
}

fn read_world(platform: &Platform, address: usize) -> Option<NativeMatrix> {
    let bytes = read_bytes::<64>(platform.read.unwrap(), address)?;
    let (words, _) = bytes.as_chunks::<4>();
    Some(std::array::from_fn(|i| f32::from_ne_bytes(words[i])))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_read_world(
    platform: &Platform,
    out: &mut MaybeUninit<NativeMatrix>,
) -> bool {
    let engine = rsf_anomaly_geometry_binding();
    let Some(world) = world_address(engine).and_then(|address| read_world(platform, address))
    else {
        return false;
    };
    out.write(world);
    true
}

fn skin_mode(platform: &Platform, skeleton: usize) -> Option<u16> {
    if skeleton == 0 {
        return None;
    }
    read_bytes(
        platform.read.unwrap(),
        skeleton.checked_add(SKELETON_MODE_OFFSET)?,
    )
    .map(u16::from_ne_bytes)
}

fn affine_matrix(native: &NativeMatrix) -> Option<Matrix> {
    if !native.iter().all(|lane| lane.is_finite())
        || [native[3], native[7], native[11], native[15]] != [0., 0., 0., 1.]
    {
        return None;
    }
    Some(from_engine_matrix(native))
}

#[repr(u8)]
#[allow(dead_code)] // The six native render trampolines supply these source tags.
enum VisualSource {
    Static = RSF_ANOMALY_TYPE_STATIC as u8,
    Progressive = RSF_ANOMALY_TYPE_PROGRESSIVE as u8,
    SkeletonSt = RSF_ANOMALY_TYPE_SKELETON_ST as u8,
    SkeletonPm = RSF_ANOMALY_TYPE_SKELETON_PM as u8,
    TreeSt = RSF_ANOMALY_TYPE_TREE_ST as u8,
    TreePm = RSF_ANOMALY_TYPE_TREE_PM as u8,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_visual_source_enter(
    platform: &Platform,
    node: *mut Visual,
    address: usize,
    source: VisualSource,
) {
    const SKELETON_ST_OFFSET: usize = 160;
    const SKELETON_PM_OFFSET: usize = 208;
    let (kind, offset) = match source {
        VisualSource::Static | VisualSource::Progressive => (RIGID, None),
        VisualSource::SkeletonSt => (HARDWARE_SKINNED, Some(SKELETON_ST_OFFSET)),
        VisualSource::SkeletonPm => (HARDWARE_SKINNED, Some(SKELETON_PM_OFFSET)),
        VisualSource::TreeSt | VisualSource::TreePm => (TREE, None),
    };
    let visual = unsafe { &mut *node };
    visual.visual = address as u64;
    visual.skeleton = offset
        .and_then(|offset| address.checked_add(offset))
        .unwrap_or(0);
    visual.kind = kind;
    visual.type_ = source as u8;
    classify_visual(platform, visual);
    unsafe { scopes::enter_visual(node) };
}

fn classify_visual(platform: &Platform, visual: &mut Visual) {
    let engine = rsf_anomaly_geometry_binding();
    let pass = read_pass(platform, engine);
    let capturing = capturing() && !scopes::in_factory();
    let probe = if capturing {
        probe_visual(&REGISTRY, visual.visual)
    } else {
        rsf_anomaly_geometry_probe::default()
    };
    let world = world_address(engine).unwrap_or(0);
    let world_for_classification = (capturing
        && probe.lookup == RSF_ANOMALY_VISUAL_MISSING as u32
        && pass != RSF_ANOMALY_PASS_SHADOW as u8)
        .then(|| read_world(platform, world))
        .flatten();
    if let Some(mode) = skin_mode(platform, visual.skeleton) {
        visual.kind = match mode {
            0 => SOFTWARE_SKINNED,
            1 => SINGLE_BONE,
            _ => HARDWARE_SKINNED,
        };
    }
    let verdict = classify(&Draw {
        pass: i32::from(pass),
        visual_type: i32::from(visual.type_),
        lookup: probe.lookup as i32,
        capturing: u32::from(capturing),
        world_readable: u32::from(world_for_classification.is_some()),
        world_identity: u32::from(
            world_for_classification
                .is_some_and(|rows| rows.map(f32::to_bits) == ENGINE_IDENTITY.map(f32::to_bits)),
        ),
        foliage_identity: u32::from(rsf_anomaly_geometry_foliage()),
    });
    visual.registry = (&raw const REGISTRY).cast();
    visual.key = probe.key;
    visual.world = world;
    visual.pass = pass;
    visual.verdict = verdict as u8;
    COUNTERS.draws[pass as usize][visual.type_ as usize][verdict as usize].increment();
    if probe.key.generation != 0 || verdict == RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE {
        COUNTERS.draws_identified.increment();
    } else {
        COUNTERS.draws_unidentified.increment();
    }
}

fn live(visual: &Visual) -> bool {
    let key = visual.key;
    // The registry is owned by the adapter or the enclosing native DrawScope.
    key.generation != 0
        && find(
            unsafe { &*visual.registry.cast::<IdentityRegistry>() },
            key.owner,
            key.visual,
        ) == key.generation
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_borrow_draw() -> *const c_void {
    // The native stack keeps the visual node alive until the hook returns.
    unsafe { scopes::visual().as_ref() }
        .filter(|visual| live(visual))
        .map_or(ptr::null(), |visual| ptr::from_ref(visual).cast())
}

fn palette(platform: &Platform, visual: &Visual) -> Option<(usize, u32)> {
    if visual.kind != HARDWARE_SKINNED {
        return None;
    }
    let mode = skin_mode(platform, visual.skeleton)?;
    let count = u32::from_ne_bytes(read_bytes(
        platform.read.unwrap(),
        visual.skeleton.checked_add(SKELETON_COUNT_OFFSET)?,
    )?);
    if !(2..=5).contains(&mode) || !(1..=MAX_CAPTURE_BONES).contains(&count) {
        return None;
    }
    let parent = read_pointer(
        platform,
        visual.skeleton.checked_add(SKELETON_PARENT_OFFSET)?,
    )?;
    if parent == 0 {
        return None;
    }
    let instances = read_pointer(platform, parent.checked_add(BONE_INSTANCES_OFFSET)?)?;
    (instances != 0).then_some((instances, count))
}

fn bone_span(instances: usize, first: usize, count: usize) -> Option<usize> {
    if instances == 0 {
        return None;
    }
    let start = instances
        .checked_add(first.checked_mul(BONE_INSTANCE_STRIDE)?)?
        .checked_add(BONE_RENDER_TRANSFORM_OFFSET)?;
    start
        .checked_add(count.checked_sub(1)?.checked_mul(BONE_INSTANCE_STRIDE)?)?
        .checked_add(size_of::<NativeMatrix>())?;
    Some(start)
}

fn write_palette(read: ReadBones, instances: usize, output: &mut [Matrix]) -> bool {
    let mut native = [[0.; 16]; BONE_BATCH];
    for (batch, output) in output.chunks_mut(BONE_BATCH).enumerate() {
        let Some(first) = bone_span(instances, batch * BONE_BATCH, output.len()) else {
            return false;
        };
        // The native leaf guards each strided read and initializes the entire
        // requested batch on success. The output remains private until finish.
        if !unsafe {
            read(
                first,
                BONE_INSTANCE_STRIDE,
                output.len() as u32,
                native.as_mut_ptr().cast(),
            )
        } {
            return false;
        }
        for (source, target) in native.iter().zip(output) {
            let Some(matrix) = affine_matrix(source) else {
                return false;
            };
            *target = matrix;
        }
    }
    true
}

struct Snapshot {
    data: *const NativeDrawCapture,
    has_world: bool,
    has_palette: bool,
}

fn snapshot(
    platform: &Platform,
    writer: &Writer,
    user: *mut c_void,
    visual: &Visual,
) -> Option<Snapshot> {
    if !live(visual) {
        return None;
    }
    let world = read_world(platform, visual.world)
        .as_ref()
        .and_then(affine_matrix);
    let (instances, count) = palette(platform, visual).unwrap_or_default();
    let storage = unsafe {
        (writer.begin.unwrap())(
            user,
            visual,
            world
                .as_ref()
                .map_or(ptr::null(), |world| ptr::from_ref(world).cast()),
            count,
        )
    };
    let layout = if count != 0 && !storage.is_null() {
        // The native writer lends count live matrices exclusively until finish.
        let output =
            unsafe { std::slice::from_raw_parts_mut(storage.cast::<Matrix>(), count as usize) };
        if write_palette(platform.bones.unwrap(), instances, output) {
            SBONES_PALETTE_LAYOUT
        } else {
            0
        }
    } else {
        0
    };
    Some(Snapshot {
        data: unsafe { (writer.finish.unwrap())(user, layout) },
        has_world: world.is_some(),
        has_palette: layout != 0,
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_capture(
    platform: &Platform,
    writer: &Writer,
    user: *mut c_void,
) -> *const NativeDrawCapture {
    unsafe { scopes::visual().as_ref() }
        .and_then(|visual| snapshot(platform, writer, user, visual))
        .map_or(ptr::null(), |capture| capture.data)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_anomaly_geometry_observe(
    platform: &Platform,
    writer: &Writer,
    user: *mut c_void,
    out: *mut Facts,
) -> *const NativeDrawCapture {
    let visual = unsafe { scopes::visual().as_ref() };
    let (view, domain) = scopes::context();
    let pass = read_pass(platform, rsf_anomaly_geometry_binding());
    let detail = scopes::in_detail();
    let mut facts = Facts {
        visual: 0,
        tree_lod: -1,
        pass,
        visual_pass: RSF_ANOMALY_PASS_MAIN as u8,
        type_: RSF_ANOMALY_TYPE_STATIC as u8,
        verdict: RSF_ANOMALY_DRAW_IDENTIFIED as u8,
        kind: RIGID,
        view,
        domain,
        in_visual: visual.is_some(),
        detail,
    };
    if let Some(visual) = visual {
        facts.visual = visual.visual;
        facts.visual_pass = visual.pass;
        facts.type_ = visual.type_;
        facts.verdict = visual.verdict;
        facts.kind = visual.kind;
        facts.view = visual.view;
        facts.domain = visual.domain;
        if visual.type_ == RSF_ANOMALY_TYPE_TREE_PM as u8 && visual.visual != 0 {
            facts.tree_lod = (visual.visual as usize)
                .checked_add(TREE_PM_LAST_LOD_OFFSET)
                .and_then(|address| read_bytes(platform.read.unwrap(), address))
                .map_or(-1, |bytes| i64::from(u32::from_ne_bytes(bytes)));
        }
    }
    // The caller provides writable output storage without initializing its fields.
    unsafe { out.write(facts) };
    if !capturing() {
        return ptr::null();
    }
    COUNTERS.flush_total[pass as usize].increment();
    if detail {
        COUNTERS.flush_in_detail[pass as usize].increment();
    }
    let Some(visual) = visual else {
        return ptr::null();
    };
    COUNTERS.flush_in_visual[pass as usize].increment();
    // A main-pass Render entry does not authorize history for a later shadow
    // or unknown flush. This phase is sampled at the verified draw call site.
    if pass != RSF_ANOMALY_PASS_MAIN as u8
        || visual.pass != RSF_ANOMALY_PASS_MAIN as u8
        || visual.verdict != RSF_ANOMALY_DRAW_IDENTIFIED as u8
    {
        return ptr::null();
    }
    let Some(capture) = snapshot(platform, writer, user, visual) else {
        COUNTERS.capture_stale.increment();
        return ptr::null();
    };
    COUNTERS.captured.increment();
    if visual.view == UNSET || visual.domain == UNSET {
        COUNTERS.capture_no_context.increment();
    }
    if !capture.has_world {
        COUNTERS.capture_no_world.increment();
    }
    if visual.kind == HARDWARE_SKINNED && !capture.has_palette {
        COUNTERS.capture_no_palette.increment();
    }
    let (user, callback) = *DRAW.read().unwrap();
    if let Some(callback) = callback {
        // The writer keeps the capture alive through this synchronous callback.
        unsafe { callback(ptr::with_exposed_provenance_mut(user), capture.data) };
        COUNTERS.sunk.increment();
    }
    capture.data
}
