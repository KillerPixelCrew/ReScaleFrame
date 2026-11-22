// SPDX-License-Identifier: MIT
//! Native CPU observations, candidate graph-to-RHI associations and capture publication.
//! The C++ capture owns guarded engine reads, typed GPU effects and narrow file effects.

pub(super) mod artifacts;
pub(crate) mod draws;
#[cfg(all(windows, not(test)))]
mod native;
pub(crate) mod root_hooks;

use crate::motion_capture_session::{CaptureInterval, is_current};
use rsf_game_sys::rsf_game_render_pass;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, c_char};
use std::mem::{offset_of, size_of};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};

const MAX_ENGINE: usize = 16384;
const MAX_ROOTS: usize = 2048;
const MAX_COMMANDS: usize = 16384;
const MAX_COMMANDS_PER_ROOT: u32 = 4096;
#[cfg(any(windows, test))]
const MAX_BLOBS: usize = 4096;
#[cfg(any(windows, test))]
const BLOB_BUDGET: usize = 32 * 1024 * 1024;

// VelocityFacts is public. The typed trampolines lend inline storage for engine and root rows.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VelocityFacts {
    fields_valid: u32,
    accepted: u32,
    visible: u32,
    mobility: u32,
    relevance: u32,
    has_velocity_called: u32,
    has_velocity: u32,
    camera_cut: u32,
    always_velocity: u32,
    history_checked: u32,
    history_found: u32,
    radius: f32,
    distance_squared: f32,
    lod_factor: f32,
    minimum_size: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EngineRecord {
    facts: VelocityFacts,
    qpc: u64,
    primitive: u64,
    proxy: u64,
    view: u64,
    interval: u32,
    thread: u32,
    component: u32,
    index: u32,
    camera: [f32; 3],
    origin: [f32; 3],
    current: [f32; 16],
    previous: [f32; 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RootRecord {
    id: u64,
    parent: u64,
    qpc: u64,
    owner: u64,
    command_list: u64,
    view: u64,
    velocity_reference: u64,
    velocity_rhi: u64,
    widget: u64,
    target: u64,
    context: u64,
    input_node: u64,
    output_node: u64,
    velocity_node: u64,
    vtable: u64,
    family: u64,
    pooled_target: u64,
    queue_begin: u64,
    queue_end: u64,
    process_fn: u64,
    desc_fn: u64,
    downsample: u64,
    blur_x: u64,
    blur_y: u64,
    glow: u64,
    execution_scope: u64,
    color_input: u64,
    color_output: u64,
    depth: u64,
    motion: u64,
    kind: u32,
    session: u32,
    interval: u32,
    thread: u32,
    fields_valid: u32,
    view_blob: u32,
    node_blob: u32,
    native_frame: u32,
    queue_uid: u32,
    output_id: u32,
    width: u32,
    height: u32,
    format: u32,
    commands: u32,
    output_valid: u32,
    native_role: u32,
    output_name: [c_char; 64],
    draw_size: [i32; 2],
}

const POSTPROCESS_BEGIN: u32 = 0;
const WIDGET_PREPARE_BEFORE: u32 = 2;
const WIDGET_QUEUE_BEFORE: u32 = 4;
const MAIN_TEMPORAL_BUILD_BEFORE: u32 = 6;
const MAIN_TEMPORAL_PROCESS_BEGIN: u32 = 8;
const GRAPH_PASS_BEGIN: u32 = 10;
#[cfg(all(windows, not(test)))]
const RHI_SCOPE_BEGIN: u32 = 12;
#[cfg(all(windows, not(test)))]
const RHI_SCOPE_RESTORE: u32 = 13;

const ROOT_NAMES: [&str; 14] = [
    "postprocess_begin",
    "postprocess_end",
    "widget_prepare_before",
    "widget_prepare_after",
    "widget_queue_before",
    "widget_queue_after",
    "main_temporal_build_before",
    "main_temporal_build_after",
    "main_temporal_process_begin",
    "main_temporal_process_end",
    "graph_pass_begin",
    "graph_pass_end",
    "rhi_scope_begin",
    "rhi_scope_restore",
];

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NativeRootScope {
    id: u64,
    view: u64,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct QueuedScope {
    scope: u64,
    view: u64,
    family: u64,
    command_list: u64,
    native_frame: u32,
    queue_uid: u32,
}

#[derive(Default)]
pub(crate) struct GeometryScope {
    native_frame: u64,
    primitive: u64,
    material: u64,
    geometry_capacity: u32,
    pub(super) role: u32,
}

struct NativeRecordCounts {
    engine: usize,
    roots: usize,
    commands: usize,
    engine_dropped: u32,
    roots_dropped: u32,
    commands_dropped: u32,
    draw_dropped: u32,
    blob_dropped: u32,
}

enum CommandAdvance {
    Stop,
    Next,
    Failed,
}

const _: () = {
    assert!(size_of::<VelocityFacts>() == 60);
    assert!(size_of::<EngineRecord>() == 264 && offset_of!(EngineRecord, previous) == 200);
    assert!(size_of::<RootRecord>() == 376 && offset_of!(RootRecord, output_name) == 304);
    assert!(align_of::<EngineRecord>() == 8 && align_of::<RootRecord>() == 8);
    assert!(size_of::<NativeRootScope>() == 16);
};

#[derive(Clone, Copy)]
struct Association {
    scope: QueuedScope,
    execute: u64,
}

#[derive(Default)]
struct Records {
    engine: Vec<EngineRecord>,
    roots: Vec<RootRecord>,
    commands: HashMap<u64, Association>,
    draws: Vec<Vec<u8>>,
    blobs: Vec<Vec<u8>>,
    #[cfg(any(windows, test))]
    blob_bytes: usize,
    engine_dropped: u32,
    roots_dropped: u32,
    commands_dropped: u32,
    draw_dropped: u32,
    blob_dropped: u32,
}

static RECORDS: LazyLock<Mutex<Records>> = LazyLock::new(|| Mutex::new(Records::default()));
static ROOT_SERIAL: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static ENGINE: Cell<*mut EngineRecord> = const { Cell::new(null_mut()) };
    static ROOT: Cell<NativeRootScope> = const { Cell::new(NativeRootScope { id: 0, view: 0 }) };
    static PASS: RefCell<Option<rsf_game_render_pass>> = const { RefCell::new(None) };
}

fn records() -> MutexGuard<'static, Records> {
    // No engine call or file effect runs under this lock.
    RECORDS.lock().unwrap_or_else(|error| error.into_inner())
}

fn observe_velocity(facts: Option<&VelocityFacts>) -> &'static CStr {
    let Some(f) = facts.filter(|f| f.fields_valid != 0) else {
        return c"unreadable";
    };
    if f.accepted != 0 {
        return c"accepted";
    }
    if f.visible == 0 {
        return c"visibility";
    }
    if !(1..=2).contains(&f.mobility) {
        return c"mobility";
    }
    if f.relevance & 0x4001 != 0x4001 {
        return c"material_or_main_pass";
    }
    let threshold = f.minimum_size * 0.02;
    if ![f.radius, f.distance_squared, f.lod_factor, threshold]
        .into_iter()
        .all(f32::is_finite)
    {
        return c"nonfinite";
    }
    if f.radius * f.radius
        <= f.distance_squared * (f.lod_factor * f.lod_factor) * (threshold * threshold)
    {
        return c"size";
    }
    if f.has_velocity_called != 0 && f.has_velocity == 0 {
        if f.camera_cut != 0 {
            return c"camera_cut";
        }
        if f.history_checked != 0 && f.history_found == 0 {
            return c"missing_history";
        }
        if f.history_found != 0 && f.always_velocity == 0 {
            return c"unchanged_transform";
        }
        return c"has_velocity_other";
    }
    c"routing_or_other"
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_ac7_capture_swap_engine(
    event: *mut EngineRecord,
) -> *mut EngineRecord {
    // The native scope lends a live stack record until it restores the previous pointer.
    ENGINE.replace(event)
}

fn with_engine_event(primitive: u64, observe: impl FnOnce(&mut EngineRecord)) {
    // SAFETY: The native scope keeps this stack record live and exclusively available to
    // these hooks during the original call, then restores the enclosing pointer on exit.
    if let Some(event) =
        unsafe { ENGINE.get().as_mut() }.filter(|event| event.primitive == primitive)
    {
        observe(event);
    }
}

type HistoryCopy = unsafe extern "C" fn(*mut f32, *const f32) -> bool;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_ac7_capture_history(
    primitive: u64,
    found: bool,
    previous: *const f32,
    copy: HistoryCopy,
) {
    with_engine_event(primitive, |event| {
        event.facts.history_checked = 1;
        event.facts.history_found = u32::from(found);
        // SAFETY: The native leaf reads the source with the existing access guard and
        // writes at most the 16 floats of the exclusively borrowed destination.
        if found && !unsafe { copy(event.previous.as_mut_ptr(), previous) } {
            event.facts.fields_valid = 0;
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_ac7_capture_has_velocity(primitive: u64, has_velocity: bool) {
    with_engine_event(primitive, |event| {
        event.facts.has_velocity_called = 1;
        event.facts.has_velocity = u32::from(has_velocity);
    });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_capture_record_engine(
    capture: CaptureInterval,
    event: *const EngineRecord,
    accepted: bool,
) -> bool {
    let mut records = records();
    if !is_current(capture.session) {
        return true;
    }
    if records.engine.len() >= MAX_ENGINE {
        records.engine_dropped = records.engine_dropped.wrapping_add(1);
    } else {
        if records.engine.try_reserve(1).is_err() {
            return false;
        }
        // SAFETY: The native detour passes its live, fully initialized record.
        let mut event = unsafe { *event };
        event.facts.accepted = u32::from(accepted);
        records.engine.push(event);
    }
    true
}

fn init_root(root: &mut RootRecord, kind: u32, owner: u64) {
    root.kind = kind;
    root.owner = owner;
    root.id = ROOT_SERIAL.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    root.parent = ROOT.get().id;
}

pub(crate) fn root_scope() -> NativeRootScope {
    ROOT.get()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_ac7_capture_enter_root(root: *const RootRecord) -> NativeRootScope {
    // SAFETY: The native detour passes its live root record.
    let root = unsafe { &*root };
    let saved = ROOT.get();
    let view = match root.kind {
        POSTPROCESS_BEGIN | MAIN_TEMPORAL_PROCESS_BEGIN | GRAPH_PASS_BEGIN => root.view,
        WIDGET_QUEUE_BEFORE => saved.view,
        _ => return saved,
    };
    ROOT.set(NativeRootScope { id: root.id, view });
    saved
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn rsf_ac7_capture_restore_root(scope: *const NativeRootScope) {
    // SAFETY: Native RAII passes the saved scope returned from enter on the same thread.
    ROOT.set(unsafe { *scope });
}

fn end_root(root: &mut RootRecord, qpc: u64) {
    root.kind = match root.kind {
        POSTPROCESS_BEGIN
        | WIDGET_PREPARE_BEFORE
        | WIDGET_QUEUE_BEFORE
        | MAIN_TEMPORAL_BUILD_BEFORE
        | MAIN_TEMPORAL_PROCESS_BEGIN
        | GRAPH_PASS_BEGIN => root.kind + 1,
        _ => return,
    };
    root.qpc = qpc;
}

fn record_root(root: &RootRecord) -> bool {
    let mut records = records();
    if !is_current(root.session) {
        return true;
    }
    if records.roots.len() >= MAX_ROOTS {
        records.roots_dropped = records.roots_dropped.wrapping_add(1);
    } else {
        if records.roots.try_reserve(1).is_err() {
            return false;
        }
        records.roots.push(*root);
    }
    true
}

fn commands_changed(root: &RootRecord, link: u64, uid: u32) -> bool {
    uid == root.queue_uid && root.queue_begin != 0 && root.queue_begin != link
}

fn associate_command(root: &mut RootRecord, command: u64, execute: u64) -> CommandAdvance {
    let mut records = records();
    if !is_current(root.session) {
        return CommandAdvance::Stop;
    }
    let association = Association {
        scope: QueuedScope {
            scope: root.id,
            view: root.view,
            family: root.family,
            command_list: root.command_list,
            native_frame: root.native_frame,
            queue_uid: root.queue_uid,
        },
        execute,
    };
    if let Some(found) = records.commands.get_mut(&command) {
        // A child completes before its parent. Keep its narrower association within the
        // same list generation and family, and replace associations after address reuse.
        if found.scope.queue_uid != root.queue_uid
            || found.scope.command_list != root.command_list
            || found.scope.family != root.family
        {
            *found = association;
        }
    } else if records.commands.len() >= MAX_COMMANDS {
        records.commands_dropped = records.commands_dropped.wrapping_add(1);
    } else {
        if records.commands.try_reserve(1).is_err() {
            return CommandAdvance::Failed;
        }
        records.commands.insert(command, association);
    }

    root.commands += 1;
    // Next is the first field, so the new tail link names the last command itself.
    if root.commands == MAX_COMMANDS_PER_ROOT || command == root.queue_end {
        CommandAdvance::Stop
    } else {
        CommandAdvance::Next
    }
}

pub(crate) fn queued_scope(command: u64, execute: u64, observed: bool) -> QueuedScope {
    let mut scope = QueuedScope::default();
    if observed
        && let Some(found) = records()
            .commands
            .get(&command)
            .filter(|found| found.execute == execute)
    {
        scope = found.scope;
    }
    PASS.with_borrow(|pass| {
        if let Some(pass) = pass.as_ref().filter(|pass| pass.scope_id != 0) {
            scope.scope = pass.scope_id;
            scope.view = pass.view_key;
            scope.family = pass.family_key;
            // The queued/root JSONL fields have always used the native 32-bit frame counter.
            scope.native_frame = pass.native_frame as u32;
        }
    });
    scope
}

/// # Safety
/// `pass` is null or a header-sized pass whose full size the header admits.
unsafe fn execution_pass(pass: *const rsf_game_render_pass) -> bool {
    // SAFETY: The callback lends a header or null, with the full pass valid when size admits it.
    if pass.is_null()
        || unsafe { pass.cast::<u32>().read() } < size_of::<rsf_game_render_pass>() as u32
    {
        PASS.replace(None);
        return false;
    }
    // SAFETY: The size check admits the full initialized pass. No saved pointer is dereferenced.
    PASS.replace(Some(unsafe { *pass }));
    true
}

fn execution_root(root: &mut RootRecord) {
    PASS.with_borrow(|pass| {
        if let Some(pass) = pass.as_ref() {
            root.owner = pass.pass_key;
            root.view = pass.view_key;
            root.family = pass.family_key;
            root.native_frame = pass.native_frame as u32;
            root.execution_scope = pass.scope_id;
            root.native_role = pass.role;
            root.color_input = pass.color_input as usize as u64;
            root.color_output = pass.color_output as usize as u64;
            root.depth = pass.depth as usize as u64;
            root.motion = pass.motion as usize as u64;
            root.width = pass.output_rect[2] as u32;
            root.height = pass.output_rect[3] as u32;
            root.fields_valid = u32::from(pass.scope_id != 0);
        }
    });
}

pub(crate) fn geometry_scope() -> GeometryScope {
    PASS.with_borrow(|pass| {
        pass.as_ref()
            .map_or_else(GeometryScope::default, |pass| GeometryScope {
                native_frame: pass.native_frame,
                primitive: pass.primitive_key,
                material: pass.material_key,
                geometry_capacity: pass.geometry_capacity,
                role: pass.role,
            })
    })
}

#[cfg(any(windows, test))]
pub(super) fn store_blob(capture: CaptureInterval, bytes: &[u8]) -> Option<u32> {
    let mut records = records();
    if bytes.is_empty() || !is_current(capture.session) {
        return Some(0);
    }
    if let Some(index) = records.blobs.iter().position(|blob| blob == bytes) {
        return Some((index + 1) as u32);
    }
    if records.blobs.len() >= MAX_BLOBS || bytes.len() > BLOB_BUDGET - records.blob_bytes {
        records.blob_dropped = records.blob_dropped.wrapping_add(1);
        return Some(0);
    }
    let mut blob = Vec::new();
    blob.try_reserve_exact(bytes.len()).ok()?;
    records.blobs.try_reserve(1).ok()?;
    blob.extend_from_slice(bytes);
    records.blob_bytes += bytes.len();
    records.blobs.push(blob);
    Some(records.blobs.len() as u32)
}

pub(super) fn reset_records() {
    let mut records = records();
    records.engine.clear();
    records.roots.clear();
    records.commands.clear();
    records.draws.clear();
    records.blobs.clear();
    #[cfg(any(windows, test))]
    {
        records.blob_bytes = 0;
    }
    records.engine_dropped = 0;
    records.roots_dropped = 0;
    records.commands_dropped = 0;
    records.draw_dropped = 0;
    records.blob_dropped = 0;
}

#[cfg(test)]
mod velocity_reason_tests {
    use super::{VelocityFacts, observe_velocity};
    use std::ffi::CStr;

    #[test]
    fn classifies_motion_capture_velocity_facts() {
        let mut facts = VelocityFacts {
            fields_valid: 1,
            accepted: 0,
            visible: 1,
            mobility: 2,
            relevance: 0x4001,
            has_velocity_called: 0,
            has_velocity: 0,
            camera_cut: 0,
            always_velocity: 0,
            history_checked: 0,
            history_found: 0,
            radius: 0.5,
            distance_squared: 10000.0,
            lod_factor: 1.0,
            minimum_size: 0.5,
        };
        let check = |facts: &VelocityFacts, expected: &CStr| {
            assert_eq!(observe_velocity(Some(facts)), expected);
        };
        check(&facts, c"size");
        facts.minimum_size = 50.0;
        facts.radius = 100.0;
        check(&facts, c"size"); // Exact equality.
        facts.minimum_size = 0.5;
        facts.radius = 2.0;
        facts.has_velocity_called = 1;
        facts.history_checked = 1;
        check(&facts, c"missing_history");
        facts.history_found = 1;
        check(&facts, c"unchanged_transform");
        facts.camera_cut = 1;
        check(&facts, c"camera_cut");
        facts.accepted = 1;
        check(&facts, c"accepted");
        facts.accepted = 0;
        facts.has_velocity_called = 0;
        facts.radius = f32::NAN;
        check(&facts, c"nonfinite");
        facts.fields_valid = 0;
        check(&facts, c"unreadable");
        facts.fields_valid = 1;
        facts.visible = 0;
        check(&facts, c"visibility");
        facts.visible = 1;
        facts.mobility = 0;
        check(&facts, c"mobility");
        facts.mobility = 2;
        facts.relevance = 1;
        check(&facts, c"material_or_main_pass");
    }
}
