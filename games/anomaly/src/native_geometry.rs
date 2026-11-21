// SPDX-License-Identifier: MIT
//! Owns geometry admission, scoped engine facts and the draw-time snapshot.
//! The native detours bracket calls with stack storage so no engine trampoline
//! or C++ unwind crosses Rust. Engine reads have separate guarded native leaves.

mod coverage;
mod scopes;
mod vertex_constants;

use crate::engine_guards::Image;
use crate::geometry_registry::{
    IdentityRegistry, Registry, rsf_anomaly_geometry_classify as classify,
    rsf_anomaly_geometry_find as find, rsf_anomaly_geometry_probe_visual as probe_visual,
    rsf_anomaly_geometry_register as register, rsf_anomaly_geometry_retire as retire,
    rsf_anomaly_geometry_retire_all as retire_all,
};
use crate::geometry_registry_abi::{
    rsf_anomaly_geometry_draw as Draw, rsf_anomaly_geometry_key as Key,
    rsf_anomaly_geometry_probe as Probe, *,
};
use crate::native_geometry_abi::*;
use coverage::COUNTERS;
use std::ffi::c_void;
use std::ptr::{self, with_exposed_provenance};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, RwLock};

const UNKNOWN_PASS: u8 = RSF_ANOMALY_PASS_UNKNOWN as u8;
const MAX_HIERARCHY: usize = 4096;
const MAX_CAPTURE_BONES: u32 = 1024;
const ENGINE_IDENTITY: [f32; 16] = [
    1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
];

static REGISTRY: IdentityRegistry = Mutex::new(Registry::new());
static CONTROL: Mutex<()> = Mutex::new(());
static MODELS: Mutex<()> = Mutex::new(());
static ENGINE: RwLock<Engine> = RwLock::new(Engine {
    module: 0,
    profile: 0,
});
static HOOKS: AtomicBool = AtomicBool::new(false);
static CAPTURE: AtomicBool = AtomicBool::new(false);
static FOLIAGE: AtomicBool = AtomicBool::new(false);
static RETIRED: CallbackSlot = CallbackSlot::new();
static DRAW: CallbackSlot = CallbackSlot::new();
static FLUSH: CallbackSlot = CallbackSlot::new();
static NEAR: CallbackSlot = CallbackSlot::new();

struct CallbackSlot(RwLock<Callback>);

impl CallbackSlot {
    const fn new() -> Self {
        Self(RwLock::new(Callback {
            function: None,
            user: 0,
            invoke: None,
        }))
    }

    fn set(&self, callback: Callback) {
        *self.0.write().unwrap() = callback;
    }

    fn call(&self, payload: *const c_void) -> bool {
        let callback = *self.0.read().unwrap();
        let (Some(function), Some(invoke)) = (callback.function, callback.invoke) else {
            return false;
        };
        // The facade supplies a typed decoder for each opaque callback and payload.
        unsafe { invoke(function, callback.user, payload) };
        true
    }
}

fn capturing() -> bool {
    CAPTURE.load(Ordering::Acquire)
}

fn fault() {
    COUNTERS.capture_faults.increment();
    CAPTURE.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_binding() -> Engine {
    *ENGINE.read().unwrap()
}

/// Admits the selected build before any detour is written. The image size
/// comes first, then every layout guard behind the native readers, then every
/// detour entry. Each site also needs its unique prefix where one is profiled.
fn guard(platform: &Platform, engine: Engine, size: u32) -> bool {
    if engine.module == 0 {
        return false;
    }
    let profile = unsafe { (platform.guard_profile)(engine) };
    if size != profile.image_size {
        return false;
    }
    let image = Image::new(with_exposed_provenance(engine.module), size, profile.read);
    let unique = profile.unique();
    // The native profile lends static NUL-terminated unique guards.
    unsafe {
        image.matches_sites(profile.layout_sites(), unique)
            && image.matches_sites(profile.sites(), unique)
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_install(platform: &Platform, engine: Engine, size: u32) -> bool {
    let _control = CONTROL.lock().unwrap();
    if HOOKS.load(Ordering::Acquire) || !guard(platform, engine, size) {
        return false;
    }
    let created = unsafe { (platform.created)() };
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
            (platform.enable)()
        } else {
            (platform.install)(engine, size)
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
    if !unsafe { (platform.created)() } {
        return false;
    }
    let disabled = unsafe { (platform.disable)() };
    if disabled {
        HOOKS.store(false, Ordering::Release);
    }
    rsf_anomaly_geometry_reset();
    disabled
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_active() -> bool {
    HOOKS.load(Ordering::Acquire) && capturing()
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_test_bind(engine: Engine) {
    *ENGINE.write().unwrap() = engine;
    CAPTURE.store(true, Ordering::Release);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_retired(callback: Callback) {
    let _models = MODELS.lock().unwrap();
    RETIRED.set(callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_draw(callback: Callback) {
    DRAW.set(callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_flush(callback: Callback) {
    FLUSH.set(callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_near(callback: Callback) {
    NEAR.set(callback);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_publish_flush(facts: *const c_void) {
    FLUSH.call(facts);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_publish_near(render: *mut c_void) {
    NEAR.call(render.cast_const());
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_set_foliage(enabled: bool) {
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
    RETIRED.call(ptr::from_ref(key).cast());
}

unsafe extern "C" fn visit_retired(_: *mut c_void, key: *const Key) {
    report_retired(unsafe { &*key });
}

fn reset_registry() {
    unsafe { retire_all(&REGISTRY, Some(visit_retired), ptr::null_mut()) };
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_reset() {
    let _models = MODELS.lock().unwrap();
    reset_registry();
}

fn read_pointer(platform: &Platform, address: usize) -> Option<usize> {
    let mut pointer = 0;
    unsafe { (platform.pointer)(address, &mut pointer) }.then_some(pointer)
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
    let mut index = 0;
    while let Some(&visual) = tree.get(index) {
        index += 1;
        let mut range = ChildRange::default();
        if !unsafe { (platform.children)(engine, visual, &mut range) } {
            return None;
        }
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
extern "C" fn rsf_anomaly_geometry_model_return(platform: &Platform, owner: usize, visual: usize) {
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
    if engine.module == 0 {
        UNKNOWN_PASS
    } else {
        unsafe { (platform.pass)(engine) }
    }
}

fn read_world(platform: &Platform, address: usize) -> Option<[f32; 16]> {
    let mut rows = [0.; 16];
    (address != 0 && unsafe { (platform.matrix)(address, &mut rows) }).then_some(rows)
}

fn affine_matrix(native: [f32; 16]) -> Option<Matrix> {
    if !native.iter().all(|lane| lane.is_finite())
        || [native[3], native[7], native[11], native[15]] != [0., 0., 0., 1.]
    {
        return None;
    }
    Some(std::array::from_fn(|index| {
        f64::from(native[index % 4 * 4 + index / 4])
    }))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_classify_visual(platform: &Platform, visual: &mut Visual) {
    let engine = rsf_anomaly_geometry_binding();
    let pass = read_pass(platform, engine);
    let capturing = capturing() && !scopes::in_factory();
    let mut probe = Probe::default();
    if capturing {
        probe_visual(&REGISTRY, visual.visual, &mut probe);
    }
    let world = unsafe { (platform.world)(engine) };
    let world_for_classification = (capturing
        && probe.lookup == RSF_ANOMALY_VISUAL_MISSING as u32
        && pass != RSF_ANOMALY_PASS_SHADOW as u8)
        .then(|| read_world(platform, world))
        .flatten();
    let mut mode = 0;
    if visual.skeleton != 0 && unsafe { (platform.skin_mode)(visual.skeleton, &mut mode) } {
        visual.kind = match mode {
            0 => SOFTWARE_SKINNED,
            1 => SINGLE_BONE,
            _ => HARDWARE_SKINNED,
        };
    }
    let verdict = classify(&Draw {
        pass: i32::from(pass),
        visual_type: i32::from(visual.visual_type),
        lookup: probe.lookup as i32,
        capturing: u32::from(capturing),
        world_readable: u32::from(world_for_classification.is_some()),
        world_identity: u32::from(
            world_for_classification
                .is_some_and(|rows| rows.map(f32::to_bits) == ENGINE_IDENTITY.map(f32::to_bits)),
        ),
        foliage_identity: u32::from(rsf_anomaly_geometry_foliage()),
    });
    visual.registry = &REGISTRY;
    visual.key = probe.key;
    visual.world = world;
    visual.pass = pass;
    visual.verdict = verdict as u8;
    COUNTERS.draws[pass as usize][visual.visual_type as usize][verdict as usize].increment();
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
        && find(unsafe { &*visual.registry }, key.owner, key.visual) == key.generation
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_borrow_draw() -> *const c_void {
    // The native stack keeps the visual and its facade alive until the hook returns.
    unsafe { scopes::visual().as_ref() }
        .filter(|visual| live(visual))
        .map_or(ptr::null(), |visual| visual.facade)
}

fn palette(platform: &Platform, visual: &Visual) -> Option<Palette> {
    let mut palette = Palette::default();
    if visual.kind != HARDWARE_SKINNED
        || visual.skeleton == 0
        || !unsafe { (platform.palette)(visual.skeleton, &mut palette) }
        || !(2..=5).contains(&palette.mode)
        || !(1..=MAX_CAPTURE_BONES).contains(&palette.count)
    {
        return None;
    }
    palette.instances = unsafe { (platform.bone_instances)(visual.skeleton) };
    (palette.instances != 0).then_some(palette)
}

fn write_palette(platform: &Platform, writer: &Writer, user: *mut c_void, palette: Palette) -> u64 {
    for index in 0..palette.count {
        let mut native = [0.; 16];
        if !unsafe { (platform.bone)(palette.instances, index, &mut native) } {
            return 0;
        }
        let Some(matrix) = affine_matrix(native) else {
            return 0;
        };
        unsafe { (writer.bone)(user, index, &matrix) };
    }
    palette.layout
}

struct Snapshot {
    data: *const c_void,
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
    let world = read_world(platform, visual.world).and_then(affine_matrix);
    let palette = palette(platform, visual);
    let count = palette.as_ref().map_or(0, |palette| palette.count);
    let has_storage = unsafe {
        (writer.begin)(
            user,
            visual,
            world.as_ref().map_or(ptr::null(), ptr::from_ref),
            count,
        )
    };
    let layout = if has_storage {
        palette.map_or(0, |palette| write_palette(platform, writer, user, palette))
    } else {
        0
    };
    Some(Snapshot {
        data: unsafe { (writer.finish)(user, layout) },
        has_world: world.is_some(),
        has_palette: layout != 0,
    })
}

#[unsafe(no_mangle)]
extern "C" fn rsf_anomaly_geometry_capture(
    platform: &Platform,
    writer: &Writer,
    user: *mut c_void,
) -> *const c_void {
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
) -> *const c_void {
    let visual = unsafe { scopes::visual().as_ref() };
    let (view, domain) = scopes::context();
    let pass = read_pass(platform, rsf_anomaly_geometry_binding());
    let detail = scopes::in_detail();
    let mut facts = Facts {
        visual: 0,
        tree_lod: -1,
        pass,
        visual_pass: RSF_ANOMALY_PASS_MAIN as u8,
        visual_type: RSF_ANOMALY_TYPE_STATIC as u8,
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
        facts.visual_type = visual.visual_type;
        facts.verdict = visual.verdict;
        facts.kind = visual.kind;
        facts.view = visual.view;
        facts.domain = visual.domain;
        if visual.visual_type == RSF_ANOMALY_TYPE_TREE_PM as u8 && visual.visual != 0 {
            facts.tree_lod = unsafe { (platform.tree_lod)(visual.visual as usize) };
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
    if DRAW.call(capture.data) {
        COUNTERS.sunk.increment();
    }
    capture.data
}
