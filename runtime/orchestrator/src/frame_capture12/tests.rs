// SPDX-License-Identifier: MIT
use super::*;
use crate::capture_test_sink::Sink;
use crate::com_ref::test::{Fake, fake, raw};
use crate::frame_capture_vk::general_precision;
use std::cell::RefCell;
use std::collections::HashMap;

const COPIED: u8 = 0x5a;
const NEVER_COPIED: u8 = 0xef;
const QUEUE: usize = 0x9e;
const LIST: usize = 0x11;
const RGBA8: u32 = 28;

#[derive(Default)]
struct World {
    core: bool,
    sink: Sink,
    layout: Layout,
    refuse_source: bool,
    // Boxed so each fake COM pointer stays stable while the list grows.
    #[allow(clippy::vec_box)]
    objects: Vec<Box<Fake>>,
    memory: HashMap<usize, Vec<u8>>,
    copies: Vec<(usize, usize, u32, usize)>,
    fences: HashMap<usize, u64>,
    refuse_signal: bool,
    drains: u32,
    drain_result: u64,
    refuse_map: bool,
    mapped: Vec<usize>,
    unmapped: u32,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World {
        sink: Sink { next_ticket: 7, ..Sink::default() },
        drain_result: 1,
        layout: Layout { bytes: 264, offset: 0, width: 2, height: 2, format: RGBA8, row_pitch: 256 },
        ..World::default()
    });
}

fn world<T>(f: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

// A COM object kept alive by the test world, its pointer and its reference count.
fn object() -> usize {
    world(|w| {
        let object = fake();
        let pointer = raw(&object) as usize;
        w.objects.push(object);
        pointer
    })
}

fn refs(pointer: usize) -> u32 {
    world(|w| {
        w.objects
            .iter()
            .find(|object| raw(object) as usize == pointer)
            .expect("test object")
            .refs
            .get()
    })
}

pub(super) unsafe extern "C" fn allocate(_: Handle, out: *mut Layout) -> Handle {
    if world(|w| w.refuse_source) {
        return std::ptr::null_mut();
    }
    let buffer = object();
    world(|w| {
        unsafe { out.write(w.layout) };
        w.memory
            .insert(buffer, vec![NEVER_COPIED; w.layout.bytes as usize]);
    });
    buffer as Handle
}

pub(super) unsafe extern "C" fn copy(
    list: Handle,
    source: Handle,
    state: u32,
    buffer: Handle,
    _: *const Layout,
) {
    world(|w| {
        w.copies
            .push((list as usize, source as usize, state, buffer as usize))
    });
}

pub(super) unsafe extern "C" fn map(buffer: Handle, bytes: u64) -> *const u8 {
    world(|w| {
        if w.refuse_map {
            return std::ptr::null();
        }
        w.mapped.push(buffer as usize);
        let memory = w.memory.get(&(buffer as usize)).expect("live buffer");
        assert_eq!(memory.len() as u64, bytes);
        memory.as_ptr()
    })
}

pub(super) unsafe extern "C" fn unmap(_: Handle) {
    world(|w| w.unmapped += 1);
}

pub(super) unsafe extern "C" fn signal(queue: Handle) -> Handle {
    assert_eq!(queue as usize, QUEUE);
    if world(|w| w.refuse_signal) {
        return std::ptr::null_mut();
    }
    let fence = object();
    world(|w| w.fences.insert(fence, 0));
    fence as Handle
}

pub(super) unsafe extern "C" fn completed(fence: Handle) -> u64 {
    world(|w| w.fences[&(fence as usize)])
}

pub(super) unsafe extern "C" fn drain(queue: Handle) -> u64 {
    assert_eq!(queue as usize, QUEUE);
    world(|w| {
        w.drains += 1;
        w.drain_result
    })
}

pub(super) unsafe extern "C" fn wants(_: *const Record) -> i32 {
    world(|w| i32::from(w.core))
}

pub(super) unsafe extern "C" fn reserve(
    record: *const Record,
    name: *const c_char,
    description: *const c_char,
    view: u32,
    scale: f32,
) -> u64 {
    world(|w| unsafe { w.sink.reserve(record, name, description, view, scale) })
}

pub(super) unsafe extern "C" fn failed(ticket: u64, name: *const c_char, result: i32) {
    world(|w| unsafe { w.sink.failed(ticket, name, result) });
}

pub(super) unsafe extern "C" fn pixels(
    ticket: u64,
    name: *const c_char,
    pixels: *const Pixels,
    api: u32,
    native: u32,
) -> i32 {
    world(|w| unsafe { w.sink.pixels(ticket, name, pixels, api, native) })
}

// The fakes frame_capture12.rs calls in place of native::ops.
pub(super) mod ops {
    pub(in crate::frame_capture12) use super::{allocate, completed, copy, drain, failed, map, pixels, reserve, signal, unmap, wants};
}

fn owner() -> Owner {
    Owner::new(Legacy::default(), false)
}

fn frame(id: u64) -> Record {
    let mut record: Record = unsafe { std::mem::zeroed() };
    record.session_id = 2;
    record.frame_id = id;
    record.view_id = 1;
    record.resource_generation = 3;
    record
}

fn record_as(owner: &Owner, source: usize, identity: &Record, role: &CStr, slot: u32) -> bool {
    owner.record(
        LIST as Handle,
        source as Handle,
        0x40,
        identity,
        role,
        slot,
        0,
        NO_RESET,
        DEFAULT_VIEW,
        0.0,
    )
}

fn record(owner: &Owner, source: usize, role: &CStr, slot: u32) -> bool {
    record_as(owner, source, &frame(9), role, slot)
}

fn poll(owner: &Owner) -> bool {
    let mut state = owner.lock();
    owner.flush(&mut state, ALL_SLOTS, true)
}

fn flush(owner: &Owner, slot: u32) -> bool {
    let mut state = owner.lock();
    owner.flush(&mut state, slot, false)
}

fn pending(owner: &Owner) -> usize {
    owner.lock().pending.len()
}

fn buffer(owner: &Owner, index: usize) -> usize {
    owner.lock().pending[index].buffer.as_ptr() as usize
}

fn fence(owner: &Owner, index: usize) -> Option<usize> {
    owner.lock().pending[index]
        .completion
        .as_ref()
        .map(|fence| fence.as_ptr() as usize)
}

// The fake GPU writes copied bytes only when a test says the copy executed.
fn execute(buffer: usize) {
    world(|w| w.memory.get_mut(&buffer).unwrap().fill(COPIED));
}

fn complete(fence: usize, value: u64) {
    world(|w| *w.fences.get_mut(&fence).unwrap() = value);
}

fn core() {
    world(|w| w.core = true);
}

#[test]
fn unrequested_frame_does_no_gpu_work() {
    let owner = owner();
    let source = object();
    assert!(!record(&owner, source, c"sr-depth", 0));
    assert_eq!(pending(&owner), 0);
    assert_eq!(refs(source), 1);
    world(|w| assert!(w.copies.is_empty() && w.sink.reserved.is_empty() && w.memory.is_empty()));
}

#[test]
fn refused_sources_reserve_nothing() {
    core();
    let owner = owner();
    let source = object();
    world(|w| w.refuse_source = true);
    assert!(!record(&owner, source, c"sr-depth", 0));
    world(|w| w.refuse_source = false);
    assert!(!owner.record(
        std::ptr::null_mut(),
        source as Handle,
        0,
        &frame(9),
        c"sr-depth",
        0,
        0,
        NO_RESET,
        DEFAULT_VIEW,
        0.0
    ));
    world(|w| assert!(w.sink.reserved.is_empty() && w.copies.is_empty()));
    assert_eq!(refs(source), 1);
}

#[test]
fn refused_reservation_releases_the_readback() {
    core();
    world(|w| w.sink.reserve_none = true);
    let owner = owner();
    let source = object();
    assert!(!record(&owner, source, c"sr-depth", 0));
    let buffers: Vec<usize> = world(|w| w.memory.keys().copied().collect());
    assert_eq!(buffers.len(), 1);
    for buffer in buffers {
        assert_eq!(refs(buffer), 0);
    }
    assert_eq!(refs(source), 1);
    world(|w| assert!(w.copies.is_empty() && w.sink.failed.is_empty()));
}

#[test]
fn core_ticket_waits_for_its_submitted_fence() {
    core();
    let owner = owner();
    let source = object();
    assert!(record(&owner, source, c"sr-linear-color", 1));
    let readback = buffer(&owner, 0);
    world(|w| {
        assert_eq!(
            w.sink.reserved,
            vec![(8, "sr_color".into(), "sr-linear-color".into(), 3, 1.0)]
        );
        assert_eq!(w.copies, vec![(LIST, source, 0x40, readback)]);
    });
    assert_eq!(refs(source), 2, "the source is leased until retirement");
    // An unsubmitted copy has no completion proof.
    assert!(poll(&owner));
    assert!(owner.submitted(QUEUE as Handle, 1, None));
    let fence = fence(&owner, 0).unwrap();
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty() && w.sink.published.is_empty()));
    execute(readback);
    complete(fence, 1);
    assert!(poll(&owner));
    world(|w| {
        assert_eq!(w.sink.published.len(), 1);
        assert_eq!(w.sink.storage, [(D3D12_API, RGBA8, 2, 2)]);
        assert_eq!(w.sink.identities, [(2, 9, 1, 3)]);
        let (ticket, name, format, pitch, size, bytes) = &w.sink.published[0];
        assert_eq!(
            (*ticket, name.as_str(), *format, *pitch, *size),
            (8, "sr_color", RGBA8, 256, 264)
        );
        assert!(bytes.iter().all(|&b| b == COPIED));
        assert_eq!(w.unmapped, 1);
    });
    assert_eq!(pending(&owner), 0);
    assert_eq!((refs(source), refs(readback), refs(fence)), (1, 0, 0));
}

#[test]
fn published_pixels_start_at_the_footprint_offset() {
    core();
    world(|w| w.layout.offset = 8);
    let owner = owner();
    assert!(record(&owner, object(), c"color", 0));
    let readback = buffer(&owner, 0);
    world(|w| w.memory.get_mut(&readback).unwrap()[8] = 1);
    assert!(flush(&owner, 0));
    world(|w| {
        let (_, _, _, _, size, bytes) = &w.sink.published[0];
        assert_eq!((*size, bytes[0]), (256, 1));
    });
}

#[test]
fn submission_fences_only_the_matching_slot_and_identity() {
    core();
    let owner = owner();
    let mut other = frame(9);
    other.resource_generation = 4;
    assert!(record(&owner, object(), c"a", 1));
    assert!(record(&owner, object(), c"b", 2));
    assert!(record_as(&owner, object(), &other, c"c", 1));
    // Nothing matches: no fence is created.
    assert!(owner.submitted(QUEUE as Handle, 3, None));
    assert!(owner.submitted(std::ptr::null_mut(), 1, None));
    world(|w| assert!(w.fences.is_empty()));
    assert!(owner.submitted(QUEUE as Handle, 1, Some(&frame(9))));
    let first = fence(&owner, 0).unwrap();
    assert_eq!((fence(&owner, 1), fence(&owner, 2)), (None, None));
    // A submitted copy keeps its first proof.
    assert!(owner.submitted(QUEUE as Handle, ALL_SLOTS, None));
    let second = fence(&owner, 1).unwrap();
    assert_eq!(fence(&owner, 0), Some(first));
    assert_eq!(fence(&owner, 2), Some(second));
    assert_eq!(refs(second), 2);
    world(|w| w.refuse_signal = true);
    assert!(record(&owner, object(), c"d", 1));
    assert!(!owner.submitted(QUEUE as Handle, 1, None));
    assert_eq!(fence(&owner, 3), None);
}

#[test]
fn removed_device_fails_the_ticket_without_mapping() {
    core();
    let owner = owner();
    let source = object();
    assert!(record(&owner, source, c"sr-depth", 0));
    assert!(owner.submitted(QUEUE as Handle, ALL_SLOTS, None));
    complete(fence(&owner, 0).unwrap(), u64::MAX);
    assert!(!poll(&owner));
    world(|w| {
        assert_eq!(w.sink.failed, vec![(8, "sr_depth".into(), MAP_FAILED)]);
        assert!(w.mapped.is_empty() && w.sink.published.is_empty());
    });
    assert_eq!((pending(&owner), refs(source)), (0, 1));
}

#[test]
fn slot_flush_publishes_only_that_slot_on_caller_proof() {
    core();
    let owner = owner();
    assert!(record(&owner, object(), c"a", 0));
    assert!(record(&owner, object(), c"b", 1));
    assert!(flush(&owner, 1));
    world(|w| {
        assert_eq!(
            (w.sink.published.len(), w.sink.published[0].1.as_str()),
            (1, "b")
        )
    });
    assert_eq!(pending(&owner), 1);
    assert!(flush(&owner, ALL_SLOTS));
    assert_eq!(pending(&owner), 0);
}

#[test]
fn abandon_fails_only_unsubmitted_copies_of_the_slot() {
    core();
    let owner = owner();
    assert!(record(&owner, object(), c"kept", 2));
    assert!(owner.submitted(QUEUE as Handle, 2, None));
    assert!(record(&owner, object(), c"lost", 2));
    assert!(record(&owner, object(), c"other", 3));
    let lost = buffer(&owner, 1);
    owner.abandon(2);
    world(|w| assert_eq!(w.sink.failed, vec![(9, "lost".into(), MAP_FAILED)]));
    assert_eq!(refs(lost), 0);
    let names: Vec<String> = owner
        .lock()
        .pending
        .iter()
        .map(|v| v.role.clone())
        .collect();
    assert_eq!(names, ["kept", "other"]);
    world(|w| assert!(w.mapped.is_empty()));
}

#[test]
fn map_failure_keeps_the_readback_for_a_later_flush() {
    core();
    let owner = owner();
    assert!(record(&owner, object(), c"color", 0));
    world(|w| w.refuse_map = true);
    assert!(!flush(&owner, ALL_SLOTS));
    assert_eq!(pending(&owner), 1);
    world(|w| assert!(w.sink.failed.is_empty() && w.sink.published.is_empty()));
    world(|w| w.refuse_map = false);
    assert!(flush(&owner, ALL_SLOTS));
    world(|w| assert_eq!(w.sink.published.len(), 1));
}

#[test]
fn preview_overrides_and_long_names_reach_the_reservation() {
    core();
    let owner = owner();
    let source = object();
    assert!(owner.record(
        LIST as Handle,
        source as Handle,
        0,
        &frame(9),
        c"fg-generated-before-ui",
        0,
        2,
        1,
        5,
        0.25
    ));
    let long = CString::new("r".repeat(300)).unwrap();
    assert!(record(&owner, source, &long, 0));
    world(|w| {
        assert_eq!(
            w.sink.reserved[0],
            (
                8,
                "fg_generated_before_ui_2".into(),
                "fg-generated-before-ui".into(),
                5,
                0.25
            )
        );
        assert_eq!(w.sink.reserved[1].1, "r".repeat(300));
        assert_eq!((w.sink.reserved[1].3, w.sink.reserved[1].4), (3, 1.0));
    });
}

fn directory(tag: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("rsf-capture12-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    directory
}

fn configured(tag: &str) -> (Owner, PathBuf) {
    let directory = directory(tag);
    let owner = owner();
    let path = CString::new(directory.to_str().unwrap()).unwrap();
    assert!(owner.configure(&path, 9, 1));
    (owner, directory)
}

#[test]
fn configuration_requires_an_absolute_bounded_idle_window() {
    let owner = owner();
    assert!(!owner.lock().legacy.enabled());
    assert!(!owner.configure(c"relative", 9, 1));
    let directory = directory("configure");
    let path = CString::new(directory.to_str().unwrap()).unwrap();
    assert!(!owner.configure(&path, 9, 0));
    assert!(!owner.configure(&path, 9, 17));
    assert!(!owner.configure(c"", 9, 1));
    assert!(owner.configure(&path, 9, 16));
    assert!(directory.is_dir());
    let state = owner.lock();
    assert!(state.legacy.enabled() && state.legacy.wants(24) && !state.legacy.wants(25));
    assert!(!state.legacy.wants(8));
    drop(state);
    assert!(record(&owner, object(), c"x", 0));
    assert!(
        !owner.configure(&path, 1, 1),
        "pending readbacks keep their window"
    );
    assert!(flush(&owner, ALL_SLOTS));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn reset_request_follows_the_legacy_window() {
    let owner = Owner::new(
        Legacy {
            directory: Some(directory("reset")),
            first: 9,
            count: 1,
        },
        true,
    );
    let wants = |frame| unsafe { rsf_core_capture12_force_reset(&owner, frame) } != 0;
    assert!(wants(9) && !wants(10));
}

#[test]
fn legacy_files_keep_the_native_footprint_and_identity() {
    let (owner, directory) = configured("legacy");
    let mut identity = frame(9);
    identity.flags = 4;
    identity.frame_time_ms = 16.666666;
    identity.camera.render_width = 640;
    identity.camera.jitter_pixels = [0.25, -0.5];
    identity.camera.view_to_clip[0] = 0.1;
    assert!(owner.record(
        LIST as Handle,
        object() as Handle,
        0,
        &identity,
        c"fg-generated-before-ui",
        0,
        1,
        0,
        DEFAULT_VIEW,
        0.0
    ));
    // Legacy-only readbacks reserve no core ticket.
    world(|w| assert!(w.sink.reserved.is_empty()));
    assert!(owner.submitted(QUEUE as Handle, ALL_SLOTS, None));
    execute(buffer(&owner, 0));
    complete(fence(&owner, 0).unwrap(), 1);
    assert!(poll(&owner));
    let raw = fs::read(directory.join("2-9-v1-g3-fg-generated-before-ui-1.raw")).unwrap();
    assert_eq!(raw, vec![COPIED; 264]);
    let json =
        fs::read_to_string(directory.join("2-9-v1-g3-fg-generated-before-ui-1.json")).unwrap();
    assert!(json.starts_with(
        "{\n\"role\":\"fg-generated-before-ui\",\n\"frame\":9,\n\"session\":2,\n\"view\":1,\n\"generation\":3,\n\"flags\":4,\n\"frame_time_ms\":16.666666,\n\"width\":2,\n\"height\":2,\n\"row_pitch\":256,\n\"offset\":0,\n\"dxgi_format\":28,\n\"bytes\":264,\n\"render_size\":[640,0],\n\"output_size\":[0,0],\n\"jitter_pixels\":[0.25,-0.5],\n\"matrix_convention\":\"row_vectors\",\n\"view_to_clip\":[0.100000001,0,"
    ), "{json}");
    assert!(json.ends_with(
        ",\n\"clip_to_previous_clip\":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],\n\"sdk_dispatch_reset\":0\n}\n"
    ));
    world(|w| assert_eq!(w.unmapped, 1));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn core_and_legacy_publish_once_from_one_mapping() {
    core();
    let (owner, directory) = configured("both");
    assert!(record(&owner, object(), c"sr-depth", 0));
    assert!(flush(&owner, 0));
    world(|w| {
        assert_eq!(
            (w.sink.published.len(), w.mapped.len(), w.unmapped),
            (1, 1, 1)
        )
    });
    let json = fs::read_to_string(directory.join("2-9-v1-g3-sr-depth-0.json")).unwrap();
    assert!(
        json.ends_with("0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]\n}\n"),
        "no reset key"
    );
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn events_are_written_only_inside_the_window() {
    let (owner, directory) = configured("event");
    let mut identity = frame(10);
    assert!(owner.event(&identity, c"sr-evaluate", 4, true, -3));
    assert!(!directory.join("metadata").exists());
    identity.frame_id = 9;
    identity.frame_time_ms = 16.666666;
    assert!(owner.event(&identity, c"sr-evaluate", 4, true, -3));
    let json = fs::read_to_string(
        directory
            .join("metadata")
            .join("2-9-v1-g3-sr-evaluate-4.json"),
    )
    .unwrap();
    assert!(json.starts_with(
        "{\"frame\":9,\"session\":2,\"view\":1,\"generation\":3,\"flags\":0,\"sdk_sequence\":4,\"frame_time_ms\":16.6667,\"reset\":1,\"result\":-3,\"cpu_ticks\":"
    ), "{json}");
    assert!(json.contains(",\"cpu_tick_frequency\":") && json.ends_with("}\n"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn queue_drain_publishes_only_copies_recorded_before_wait() {
    core();
    let owner = owner();
    assert!(owner.flush_after_queue(QUEUE as Handle, || panic!("empty batch")));
    world(|w| assert_eq!(w.drains, 0));
    assert!(record(&owner, object(), c"a", 0));
    assert!(record(&owner, object(), c"b", 1));
    assert!(!owner.flush_after_queue(QUEUE as Handle, || false));
    world(|w| assert_eq!(w.drains, 0));
    world(|w| w.drain_result = 0);
    assert!(!owner.flush_after_queue(QUEUE as Handle, || true));
    assert_eq!(pending(&owner), 2);
    world(|w| w.drain_result = u64::MAX);
    assert!(!owner.flush_after_queue(QUEUE as Handle, || true));
    assert_eq!(pending(&owner), 2);
    world(|w| assert!(w.mapped.is_empty()));
    world(|w| w.drain_result = 1);
    assert!(owner.flush_after_queue(QUEUE as Handle, || { record(&owner, object(), c"late", 2) }));
    world(|w| assert_eq!((w.drains, w.sink.published.len()), (3, 2)));
    assert_eq!(pending(&owner), 1);
    assert!(owner.flush_after_queue(QUEUE as Handle, || true));
    world(|w| assert_eq!((w.drains, w.sink.published.len()), (4, 3)));
    assert_eq!(pending(&owner), 0);
}

#[test]
fn destroy_fails_pending_tickets_and_releases_leases() {
    core();
    let owner = owner();
    let source = object();
    assert!(record(&owner, source, c"snapshot", 0));
    assert!(owner.submitted(QUEUE as Handle, ALL_SLOTS, None));
    let (readback, fence) = (buffer(&owner, 0), fence(&owner, 0).unwrap());
    drop(owner);
    world(|w| {
        assert_eq!(w.sink.failed, vec![(8, "snapshot".into(), STAGING_FAILED)]);
        assert!(w.mapped.is_empty());
    });
    assert_eq!((refs(source), refs(readback), refs(fence)), (1, 0, 0));
}

#[test]
fn null_arguments_are_refused_at_the_boundary() {
    unsafe {
        assert_eq!(rsf_core_capture12_enabled(std::ptr::null()), 0);
        let owner = owner();
        assert_eq!(
            rsf_core_capture12_configure(&owner, std::ptr::null(), 1, 1),
            0
        );
        assert_eq!(
            rsf_core_capture12_record(
                &owner,
                LIST as Handle,
                object() as Handle,
                0,
                std::ptr::null(),
                c"x".as_ptr(),
                0,
                0,
                NO_RESET,
                DEFAULT_VIEW,
                0.0
            ),
            0
        );
        assert_eq!(
            rsf_core_capture12_submitted(&owner, std::ptr::null_mut(), 0, std::ptr::null()),
            1
        );
    }
}

#[test]
fn event_frame_time_uses_default_stream_precision() {
    for (value, text) in [
        (16.666666f32, "16.6667"),
        (0.1, "0.1"),
        (1234567.0, "1.23457e+06"),
        (0.0, "0"),
    ] {
        assert_eq!(general_precision(f64::from(value), 6), text);
    }
}
