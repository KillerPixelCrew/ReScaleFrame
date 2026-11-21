// SPDX-License-Identifier: MIT
use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

const COPIED: u8 = 0x5a;
const NEVER_COPIED: u8 = 0xef;
const DEVICE: usize = 0xd0;

#[derive(Default)]
struct World {
    core: bool,
    core_frame: bool,
    next_ticket: u64,
    reserved: Vec<(u64, String, u32, f32)>,
    failed: Vec<(u64, String, i32)>,
    published: Vec<(u64, String, u32, Vec<u8>)>,
    ready: bool,
    ready_queries: u32,
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
    static WORLD: RefCell<World> = RefCell::new(World { next_ticket: 7, next_handle: 100, ..World::default() });
}

fn world<T>(f: impl FnOnce(&mut World) -> T) -> T {
    WORLD.with(|world| f(&mut world.borrow_mut()))
}

unsafe fn text(value: *const c_char) -> String {
    unsafe { CStr::from_ptr(value) }
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C" fn allocate(
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

unsafe extern "C" fn encode(_: Handle, commands: Handle, _: *const Image, _: *const Allocation) {
    world(|w| w.encoded.push(commands as usize));
}

unsafe extern "C" fn event_set(_: Handle, event: u64) -> i32 {
    world(|w| i32::from(w.events.get(&event).copied().unwrap_or(false)))
}

unsafe extern "C" fn map(_: Handle, memory: u64, _: u64, out: *mut *mut c_void) -> i32 {
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

unsafe extern "C" fn unmap(_: Handle, _: u64) {
    world(|w| w.unmapped += 1);
}

unsafe extern "C" fn release(_: Handle, allocation: *const Allocation) {
    let a = unsafe { *allocation };
    world(|w| {
        w.memory.remove(&a.memory);
        w.released.push((a.buffer, a.memory, a.event));
    });
}

unsafe extern "C" fn name(
    role: *const c_char,
    out: *mut c_char,
    capacity: u32,
    view: *mut u32,
    scale: *mut f32,
) -> i32 {
    let name = unsafe { text(role) }.replace('-', "_");
    if name.len() + 1 > capacity as usize {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(name.as_ptr().cast(), out, name.len());
        out.add(name.len()).write(0);
        view.write(3);
        scale.write(1.0);
    }
    1
}

unsafe extern "C" fn wants(_: *const Record) -> i32 {
    world(|w| i32::from(w.core))
}

unsafe extern "C" fn wants_frame(_: u64) -> i32 {
    world(|w| i32::from(w.core_frame))
}

unsafe extern "C" fn reserve(
    _: *const Record,
    name: *const c_char,
    _: *const c_char,
    view: u32,
    scale: f32,
) -> u64 {
    let name = unsafe { text(name) };
    world(|w| {
        w.next_ticket += 1;
        w.reserved.push((w.next_ticket, name, view, scale));
        w.next_ticket
    })
}

unsafe extern "C" fn failed(ticket: u64, name: *const c_char, result: i32) {
    let name = unsafe { text(name) };
    world(|w| w.failed.push((ticket, name, result)));
}

unsafe extern "C" fn pixels(
    ticket: u64,
    name: *const c_char,
    pixels: *const Pixels,
    api: u32,
    _: u32,
) -> i32 {
    let name = unsafe { text(name) };
    let pixels = unsafe { &*pixels };
    assert_eq!(api, VULKAN_API);
    let bytes =
        unsafe { std::slice::from_raw_parts(pixels.data.cast::<u8>(), pixels.data_size as usize) }
            .to_vec();
    world(|w| w.published.push((ticket, name, pixels.format, bytes)));
    1
}

unsafe extern "C" fn gpu_ready(_: u64, _: *const c_char, api: u32, device: Handle) -> i32 {
    assert_eq!((api, device as usize), (VULKAN_API, DEVICE));
    world(|w| {
        w.ready_queries += 1;
        i32::from(w.ready)
    })
}

static OPS: Platform = Platform {
    allocate,
    encode,
    event_set,
    map,
    unmap,
    release,
    name,
    wants,
    wants_frame,
    reserve,
    failed,
    pixels,
    gpu_ready,
};

fn owner(command_completion: bool, legacy: Legacy) -> Owner {
    let owner = Owner::new(command_completion, std::ptr::null_mut(), &OPS, legacy);
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
    world(|w| w.memory.get_mut(&memory).unwrap().fill(COPIED));
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
        assert!(w.encoded.is_empty() && w.reserved.is_empty() && w.next_handle == 100);
    });
}

#[test]
fn shared_ticket_waits_for_shared_completion_proof() {
    core();
    let owner = owner(false, Legacy::default());
    let (done, ticket) = record(&owner, 1, c"sr-linear-color", 0);
    assert!(done && ticket != 0);
    world(|w| assert_eq!(w.reserved, vec![(ticket, "sr_linear_color".into(), 3, 1.0)]));
    // Queries before the watermark certify the reservation must not map.
    assert!(poll(&owner));
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty() && w.published.is_empty() && w.ready_queries == 2));
    execute(pending_memory(&owner, 0));
    world(|w| w.ready = true);
    assert!(poll(&owner));
    world(|w| {
        assert_eq!(w.published.len(), 1);
        assert_eq!(w.published[0].0, ticket);
        assert_eq!(w.published[0].2, 28);
        assert!(w.published[0].3.iter().all(|&b| b == COPIED));
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
        assert_eq!(w.failed, vec![(ticket, "sr_depth".into(), MAP_FAILED)]);
        assert!(w.mapped.is_empty() && w.published.is_empty() && w.released.len() == 1);
    });
}

#[test]
fn exact_group_ignores_shared_watermark_until_completed() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"snapshot", 4);
    // A shared watermark covering the queue is not proof for this exact group.
    world(|w| w.ready = true);
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty() && w.ready_queries == 0));
    owner.complete_group(3);
    assert!(poll(&owner));
    world(|w| assert!(w.mapped.is_empty()));
    execute(pending_memory(&owner, 0));
    owner.complete_group(4);
    assert!(poll(&owner));
    world(|w| {
        assert_eq!(w.published.len(), 1);
        assert_eq!(w.published[0].0, ticket);
        assert!(w.published[0].3.iter().all(|&b| b == COPIED));
        assert_eq!(w.ready_queries, 0);
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
        assert_eq!(w.failed, vec![(ticket, "snapshot".into(), STAGING_FAILED)]);
        assert!(w.mapped.is_empty() && w.published.is_empty() && w.released.len() == 1);
    });
}

#[test]
fn incomplete_group_fails_on_full_drain_without_mapping() {
    core();
    let owner = owner(false, Legacy::default());
    let (_, ticket) = record(&owner, 1, c"snapshot", 6);
    assert!(!flush(&owner));
    world(|w| {
        assert_eq!(w.failed, vec![(ticket, "snapshot".into(), MAP_FAILED)]);
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
        assert_eq!(w.published.len(), 1);
        assert_eq!(w.published[0].0, first);
    });
    // Retiring a recording whose event never signalled abandons it unmapped.
    unsafe { rsf_core_capture_vk_retire(&owner, 12 as Handle) };
    world(|w| {
        assert_eq!(w.mapped.len(), 1);
        assert_eq!(w.failed, vec![(second, "second".into(), MAP_FAILED)]);
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
        assert_eq!(w.failed, vec![(ticket, "snapshot".into(), STAGING_FAILED)]);
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
    world(|w| assert!(w.reserved.is_empty() && w.next_handle == 100));
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
        assert_eq!(w.failed, vec![(ticket, "big".into(), UNSUPPORTED_FORMAT)]);
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
    world(|w| assert!(w.reserved.is_empty() && w.next_handle == 100));
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
        assert_eq!(w.failed, vec![(ticket, "snapshot".into(), STAGING_FAILED)]);
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
    assert!(!directory.join("2-9-fg-hudless-color-0.raw").exists());
    execute(pending_memory(&owner, 0));
    assert!(flush(&owner));
    let raw = fs::read(directory.join("2-9-fg-hudless-color-0.raw")).unwrap();
    assert_eq!(raw, vec![COPIED; 16]);
    let json = fs::read_to_string(directory.join("2-9-fg-hudless-color-0.json")).unwrap();
    assert!(json.starts_with("{\n\"role\":\"fg-hudless-color\",\n\"frame\":9,\n\"session\":2,"));
    assert!(json.contains("\"row_pitch\":8,\n\"offset\":0,\n\"vk_format\":37,\n\"bytes\":16,"));
    assert!(json.contains("\"completion\":\"owner-drained-context-retirement\",\n\"fg_backend\":3,\n\"provider_version_id\":77,"));
    assert!(json.contains("\"clip_to_previous_clip\":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]\n}\n"));
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn shared_core_and_legacy_publish_once_each() {
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
    execute(pending_memory(&owner, 0));
    world(|w| w.ready = true);
    assert!(poll(&owner));
    world(|w| assert_eq!((w.published.len(), w.published[0].0), (1, ticket)));
    assert_eq!(
        owner.lock().pending.len(),
        1,
        "legacy copy waits for the drain"
    );
    assert!(flush(&owner));
    world(|w| assert_eq!((w.published.len(), w.mapped.len()), (1, 2)));
    assert!(directory.join("2-9-sr-depth-0.raw").exists());
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
