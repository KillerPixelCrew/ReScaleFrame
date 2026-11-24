// SPDX-License-Identifier: MIT
use super::*;
use crate::capture_test_sink::Sink;
use std::cell::RefCell;
use std::collections::HashMap;

const COPIED: u8 = 0x5a;
const NEVER_COPIED: u8 = 0xef;
const DEVICE: usize = 0xd0;

#[derive(Default)]
struct World {
    core: bool,
    core_frame: bool,
    sink: Sink,
    fail_stage: Option<u32>,
    next_handle: u64,
    encoded: Vec<usize>,
    events: HashMap<u64, bool>,
    memory: HashMap<u64, Vec<u8>>,
    event_memory: HashMap<u64, u64>,
    mapped: Vec<u64>,
    unmapped: u32,
    released: Vec<(u64, u64, u64)>,
}

thread_local! {
    static WORLD: RefCell<World> = RefCell::new(World { sink: Sink { next_ticket: 7, ..Sink::default() }, next_handle: 100, ..World::default() });
}

fn world<T>(f: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

pub(super) unsafe fn allocate(
    _: Handle,
    _: *const Image,
    bytes: u64,
    event: u32,
    out: *mut Allocation,
) -> i32 {
    let out = unsafe { &mut *out };
    world(|w| {
        let mut handle = || {
            w.next_handle += 1;
            w.next_handle
        };
        out.buffer = handle();
        if w.fail_stage == Some(0) {
            return -2;
        }
        out.memory = handle();
        if w.fail_stage == Some(1) {
            return -2;
        }
        w.memory
            .insert(out.memory, vec![NEVER_COPIED; bytes as usize]);
        if event != 0 {
            out.event = handle();
            w.events.insert(out.event, false);
            w.event_memory.insert(out.event, out.memory);
        }
        0
    })
}

pub(super) unsafe fn encode(_: Handle, commands: Handle, _: *const Image, _: *const Allocation) {
    world(|w| w.encoded.push(commands as usize));
}

pub(super) unsafe fn event_set(_: Handle, event: u64) -> i32 {
    world(|w| i32::from(w.events.get(&event).copied().unwrap_or(false)))
}

pub(super) unsafe fn map(_: Handle, memory: u64, _: u64, out: *mut *mut c_void) -> i32 {
    world(|w| {
        w.mapped.push(memory);
        match w.memory.get_mut(&memory) {
            Some(bytes) => {
                unsafe { out.write(bytes.as_mut_ptr().cast()) };
                0
            }
            None => -5,
        }
    })
}

pub(super) unsafe fn unmap(_: Handle, _: u64) {
    world(|w| w.unmapped += 1);
}

pub(super) unsafe fn release(_: Handle, allocation: *const Allocation) {
    let a = unsafe { *allocation };
    world(|w| {
        w.memory.remove(&a.memory);
        w.released.push((a.buffer, a.memory, a.event));
    });
}

pub(super) unsafe fn wants(_: *const Record) -> i32 {
    world(|w| i32::from(w.core))
}

pub(super) fn wants_frame(_: u64) -> i32 {
    world(|w| i32::from(w.core_frame))
}

pub(super) unsafe fn reserve(
    record: *const Record,
    name: *const c_char,
    description: *const c_char,
    view: u32,
    scale: f32,
) -> u64 {
    world(|w| unsafe { w.sink.reserve(record, name, description, view, scale) })
}

pub(super) unsafe fn failed(ticket: u64, name: *const c_char, result: i32) {
    world(|w| unsafe { w.sink.failed(ticket, name, result) });
}

pub(super) unsafe fn pixels(
    ticket: u64,
    name: *const c_char,
    pixels: *const Pixels,
    api: u32,
    native: u32,
) -> i32 {
    world(|w| unsafe { w.sink.pixels(ticket, name, pixels, api, native) })
}

fn owner(command_completion: bool, legacy: Legacy) -> Owner {
    let owner = Owner::new(command_completion, std::ptr::null_mut(), legacy);
    owner.lock().device = DEVICE;
    owner
}

fn frame(id: u64) -> Record {
    let mut record: Record = unsafe { std::mem::zeroed() };
    record.session_id = 2;
    record.frame_id = id;
    record
}

fn image(format: u32) -> Image {
    Image {
        device: DEVICE as Handle,
        image: 1,
        format,
        width: 2,
        height: 2,
        layout: 5,
        copy_source: 1,
    }
}

fn record(owner: &Owner, commands: usize, role: &CStr, group: u64) -> (bool, u64) {
    let mut ticket = u64::MAX;
    let done = owner.record(
        commands,
        &image(37),
        &frame(9),
        role,
        0,
        true,
        &mut ticket,
        group,
    );
    (done, ticket)
}

// The fake GPU writes copied bytes only when a test says the copy executed.
fn execute(memory: u64) {
    world(|w| {
        w.memory.get_mut(&memory).unwrap().fill(COPIED);
        for (&event, &event_memory) in &w.event_memory {
            if event_memory == memory {
                w.events.insert(event, true);
            }
        }
    });
}

fn pending_memory(owner: &Owner, index: usize) -> u64 {
    owner.lock().pending[index].allocation.memory
}

fn poll(owner: &Owner) -> bool {
    let mut state = owner.lock();
    owner.wants(&mut state, 9)
}

fn flush(owner: &Owner) -> bool {
    let mut state = owner.lock();
    owner.flush(&mut state, false, 0)
}

fn core() {
    world(|w| {
        w.core = true;
        w.core_frame = true;
    });
}

#[test]
fn unrequested_frame_does_no_gpu_work() {
    let owner = owner(false, Legacy::default());
    let (done, ticket) = record(&owner, 1, c"sr-depth", 0);
    assert!(done);
    assert_eq!(ticket, 0);
    assert!(owner.lock().pending.is_empty());
    world(|w| {
        assert!(w.encoded.is_empty() && w.sink.reserved.is_empty() && w.next_handle == 100);
    });
}

#[test]
fn overflowing_capture_extent_fails_the_exact_ticket_once() {
    core();
    let owner = owner(false, Legacy::default());
    let mut image = image(37);
    image.width = u32::MAX;
    image.height = u32::MAX;
    let mut ticket = 0;
    assert!(!owner.record(
        1,
        &image,
        &frame(9),
        c"sr-motion-uv",
        0,
        true,
        &mut ticket,
        1,
    ));
    assert_ne!(ticket, 0);
    assert!(owner.lock().pending.is_empty());
    world(|w| {
        assert_eq!(
            w.sink.reserved,
            [(ticket, "sr_motion".into(), "sr-motion-uv".into(), 7, 8.0)]
        );
        assert_eq!(
            w.sink.failed,
            [(ticket, "sr_motion".into(), UNSUPPORTED_FORMAT)]
        );
        assert!(w.encoded.is_empty() && w.next_handle == 100);
    });
}

#[test]
fn ungrouped_ticket_waits_for_its_command_retirement() {
    core();
    let owner = owner(false, Legacy::default());
    let (done, ticket) = record(&owner, 1, c"sr-linear-color", 0);
    assert!(done && ticket != 0);
    world(|w| {
        assert_eq!(
            w.sink.reserved,
            vec![(ticket, "sr_color".into(), "sr-linear-color".into(), 3, 1.0)]
        )
    });
    // Polling without a retirement proof does not map an ungrouped copy.
    assert!(poll(&owner));
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty() && w.sink.published.is_empty()));
    execute(pending_memory(&owner, 0));
    unsafe { rsf_core_capture_vk_retire(&owner, 1 as Handle) };
    world(|w| {
        assert_eq!(w.sink.published.len(), 1);
        assert_eq!(w.sink.published[0].0, ticket);
        assert_eq!(w.sink.published[0].2, 28);
        assert_eq!(w.sink.storage, [(VULKAN_API, 37, 2, 2)]);
        assert_eq!(w.sink.identities, [(2, 9, 0, 0)]);
        assert!(w.sink.published[0].5.iter().all(|&b| b == COPIED));
        assert_eq!((w.released.len(), w.unmapped), (1, 1));
    });
    assert!(owner.lock().pending.is_empty());
}

#[test]
fn shared_ticket_without_proof_fails_on_full_drain() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"sr-depth", 0);
    assert!(!flush(&owner));
    world(|w| {
        assert_eq!(w.sink.failed, vec![(ticket, "sr_depth".into(), MAP_FAILED)]);
        assert!(w.mapped.is_empty() && w.sink.published.is_empty() && w.released.len() == 1);
    });
}

#[test]
fn exact_group_waits_for_its_own_fence() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"snapshot", 4);

    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty()));
    owner.complete_group(3);
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty()));
    execute(pending_memory(&owner, 0));
    owner.complete_group(4);
    assert!(poll(&owner));
    world(|w| {
        assert_eq!(w.sink.published.len(), 1);
        assert_eq!(w.sink.published[0].0, ticket);
        assert!(w.sink.published[0].5.iter().all(|&b| b == COPIED));
    });
}

#[test]
fn discarded_group_fails_once_and_never_maps() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"snapshot", 5);
    owner.discard_group(5);
    // Completion after refusal cannot restore publication.
    owner.complete_group(5);
    owner.discard_group(5);
    assert!(poll(&owner));
    assert_eq!(owner.lock().pending.len(), 1, "allocation kept until drain");
    assert!(flush(&owner));
    world(|w| {
        assert_eq!(
            w.sink.failed,
            vec![(ticket, "snapshot".into(), STAGING_FAILED)]
        );
        assert!(w.mapped.is_empty() && w.sink.published.is_empty() && w.released.len() == 1);
    });
}

#[test]
fn incomplete_group_fails_on_full_drain_without_mapping() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"snapshot", 6);
    assert!(!flush(&owner));
    world(|w| {
        assert_eq!(w.sink.failed, vec![(ticket, "snapshot".into(), MAP_FAILED)]);
        assert!(w.mapped.is_empty() && w.released.len() == 1);
    });
}

#[test]
fn command_retirement_matches_exact_executed_commands() {
    core();
    let owner = owner(true, Legacy::default());
    let (_, first) = record(&owner, 11, c"first", 0);
    let (_, second) = record(&owner, 12, c"second", 0);
    let first_memory = pending_memory(&owner, 0);
    // Ready-only polling never maps command-retirement readbacks.
    {
        let mut state = owner.lock();
        assert!(owner.flush(&mut state, true, 0));
    }
    let event = owner.lock().pending[0].allocation.event;
    execute(first_memory);
    world(|w| *w.events.get_mut(&event).unwrap() = true);
    unsafe { rsf_core_capture_vk_retire(&owner, 11 as Handle) };
    world(|w| {
        assert_eq!(w.mapped, vec![first_memory]);
        assert_eq!(w.sink.published.len(), 1);
        assert_eq!(w.sink.published[0].0, first);
    });
    // Retiring a recording whose event never signalled abandons it unmapped.
    unsafe { rsf_core_capture_vk_retire(&owner, 12 as Handle) };
    world(|w| {
        assert_eq!(w.mapped.len(), 1);
        assert_eq!(w.sink.failed, vec![(second, "second".into(), MAP_FAILED)]);
        assert_eq!(w.released.len(), 2);
    });
    assert!(owner.lock().pending.is_empty());
}

#[test]
fn partial_allocation_is_retained_then_refused() {
    core();
    world(|w| w.fail_stage = Some(1));
    let owner = owner(false, Legacy::default());
    let (done, ticket) = record(&owner, 1, c"snapshot", 0);
    assert!(!done && ticket != 0);
    assert_eq!(owner.lock().pending.len(), 1);
    world(|w| assert!(w.encoded.is_empty()));
    assert!(poll(&owner));
    world(|w| {
        assert_eq!(
            w.sink.failed,
            vec![(ticket, "snapshot".into(), STAGING_FAILED)]
        );
        assert_eq!(w.released, vec![(101, 102, 0)]);
        assert!(w.mapped.is_empty());
    });
}

#[test]
fn refused_sources_reserve_nothing() {
    core();
    let owner = owner(false, Legacy::default());
    let mut source = image(37);
    source.copy_source = 0;
    let mut ticket = 0;
    assert!(!owner.record(1, &source, &frame(9), c"x", 0, true, &mut ticket, 0));
    source = image(1);
    assert!(!owner.record(1, &source, &frame(9), c"x", 0, true, &mut ticket, 0));
    source = image(37);
    source.device = 0xbad as Handle;
    assert!(!owner.record(1, &source, &frame(9), c"x", 0, true, &mut ticket, 0));
    assert!(!owner.record(0, &image(37), &frame(9), c"x", 0, true, &mut ticket, 0));
    world(|w| assert!(w.sink.reserved.is_empty() && w.next_handle == 100));
}

#[test]
fn oversized_reservation_fails_unsupported() {
    core();
    let owner = owner(false, Legacy::default());
    let mut source = image(97);
    source.width = 8192;
    source.height = 8192;
    let mut ticket = 0;
    assert!(!owner.record(1, &source, &frame(9), c"big", 0, true, &mut ticket, 0));
    world(|w| {
        assert_eq!(
            w.sink.failed,
            vec![(ticket, "big".into(), UNSUPPORTED_FORMAT)]
        );
        assert_eq!(w.next_handle, 100);
    });
}

#[test]
fn non_core_source_skips_reservation() {
    core();
    let owner = owner(false, Legacy::default());
    let mut ticket = 1;
    assert!(owner.record(1, &image(37), &frame(9), c"x", 0, false, &mut ticket, 0));
    assert_eq!(ticket, 0);
    world(|w| assert!(w.sink.reserved.is_empty() && w.next_handle == 100));
}

#[test]
fn destroy_fails_pending_tickets_and_releases_without_mapping() {
    core();
    let owner = owner(false, Legacy::default());
    let (accepted, ticket) = record(&owner, 1, c"snapshot", 0);
    assert!(accepted && ticket != 0);
    drop(owner);
    world(|w| {
        assert!(w.mapped.is_empty() && w.released.len() == 1);
        assert_eq!(
            w.sink.failed,
            vec![(ticket, "snapshot".into(), STAGING_FAILED)]
        );
    });
}

fn legacy_directory(tag: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("rsf-capture-vk-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn legacy_publishes_only_after_drain() {
    let directory = legacy_directory("legacy");
    let owner = owner(
        false,
        Legacy {
            directory: Some(directory.clone()),
            first: 9,
            count: 1,
        },
    );
    unsafe { rsf_core_capture_vk_provider(&owner, 77, 3, 1) };
    let (done, ticket) = record(&owner, 1, c"fg-hudless-color", 0);
    assert!(done && ticket == 0);
    // Shared polling never writes a legacy-only readback.
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty()));
    assert!(!directory.join("2-9-v0-g0-fg-hudless-color-0.raw").exists());
    execute(pending_memory(&owner, 0));
    assert!(flush(&owner));
    let raw = fs::read(directory.join("2-9-v0-g0-fg-hudless-color-0.raw")).unwrap();
    assert_eq!(raw, vec![COPIED; 16]);
    let json = fs::read_to_string(directory.join("2-9-v0-g0-fg-hudless-color-0.json")).unwrap();
    assert!(json.starts_with("{\n\"role\":\"fg-hudless-color\",\n\"frame\":9,\n\"session\":2,"));
    assert!(json.contains("\"row_pitch\":8,\n\"offset\":0,\n\"vk_format\":37,\n\"bytes\":16,"));
    assert!(json.contains("\"completion\":\"owner-drained-context-retirement\",\n\"fg_backend\":3,\n\"provider_version_id\":77,"));
    assert!(json.contains("\"clip_to_previous_clip\":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]\n}\n"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn core_and_legacy_use_the_same_execution_proof() {
    core();
    let directory = legacy_directory("both");
    let owner = owner(
        false,
        Legacy {
            directory: Some(directory.clone()),
            first: 9,
            count: 1,
        },
    );
    let (_, ticket) = record(&owner, 1, c"sr-depth", 0);
    assert!(poll(&owner));
    assert!(!flush(&owner));
    world(|w| {
        assert_eq!(w.sink.failed, [(ticket, "sr_depth".into(), MAP_FAILED)]);
        assert!(w.mapped.is_empty());
    });
    assert!(!directory.join("2-9-v0-g0-sr-depth-0.raw").exists());
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn legacy_artifacts_distinguish_views_generations_and_repeated_evaluations() {
    let directory = legacy_directory("identity");
    let owner = owner(
        false,
        Legacy {
            directory: Some(directory.clone()),
            first: 9,
            count: 1,
        },
    );
    let mut identity = frame(9);
    for (view, generation) in [(1, 2), (2, 2), (1, 3), (1, 2)] {
        identity.view_id = view;
        identity.resource_generation = generation;
        assert!(owner.record(
            3,
            &image(37),
            &identity,
            c"role\"quote",
            0,
            false,
            &mut 0,
            0
        ));
    }
    for index in 0..4 {
        execute(pending_memory(&owner, index));
    }
    assert!(flush(&owner));
    for stem in [
        "v1-g2-role%22quote-0",
        "v2-g2-role%22quote-0",
        "v1-g3-role%22quote-0",
        "v1-g2-role%22quote-0-repeat1",
    ] {
        let json = fs::read_to_string(directory.join(format!("2-9-{stem}.json"))).unwrap();
        assert!(json.contains(r#""role":"role\"quote""#));
        assert!(directory.join(format!("2-9-{stem}.raw")).is_file());
    }
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn legacy_failure_log_names_frame_and_role() {
    let directory = legacy_directory("errors");
    let owner = owner(
        false,
        Legacy {
            directory: Some(directory.clone()),
            first: 9,
            count: 1,
        },
    );
    let mut source = image(37);
    source.copy_source = 0;
    let mut ticket = 0;
    assert!(!owner.record(1, &source, &frame(9), c"color", 0, true, &mut ticket, 0));
    let log = fs::read_to_string(directory.join("capture-errors.log")).unwrap();
    assert_eq!(log, "9 color\n");
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn general_matches_iostream_precision_nine() {
    assert_eq!(general(f64::NAN.copysign(-1.0)), "-nan");
    for (value, text) in [
        (0.0, "0"),
        (1.0, "1"),
        (-2.5, "-2.5"),
        (0.1f32 as f64, "0.100000001"),
        (16.666666f32 as f64, "16.666666"),
        (1e-5, "1e-05"),
        (123456789.0, "123456789"),
        (1234567890.0, "1.23456789e+09"),
        (0.0001, "0.0001"),
        (f32::MAX as f64, "3.40282347e+38"),
    ] {
        assert_eq!(general(value), text, "{value}");
    }
}

#[test]
fn counts_parse_like_wcstoull() {
    use std::ffi::OsStr;
    assert_eq!(parse_count(OsStr::new("12")), 12);
    assert_eq!(parse_count(OsStr::new("  7x")), 7);
    assert_eq!(parse_count(OsStr::new("abc")), 0);
    assert_eq!(parse_count(OsStr::new("-1")), u64::MAX);
    assert_eq!(parse_count(OsStr::new("99999999999999999999999")), u64::MAX);
    assert_eq!(parse_count(OsStr::new(&"1".repeat(32))), 0);
}
