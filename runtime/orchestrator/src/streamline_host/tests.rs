// SPDX-License-Identifier: MIT
use super::*;
use std::cell::{Cell, RefCell};

const PING: u32 = 0xc123;

thread_local! {
    static SLEEPS: Cell<u32> = const { Cell::new(0) };
    static MARKERS: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    static STARTUP: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn interposer_loaded() -> u32 {
    0
}
unsafe extern "C" fn adapter_vendor(_: Handle, vendor: &mut u32, _: &mut Handle) -> u32 {
    *vendor = 0x10de;
    1
}
unsafe extern "C" fn report_vendor(_: Log, _: Handle, _: u32) {}
unsafe extern "C" fn load(_: *const u16, _: &Setup) -> Handle {
    STARTUP.with_borrow_mut(|calls| calls.push("load"));
    std::ptr::without_provenance_mut(0x1000)
}
unsafe extern "C" fn unload(module: Handle) {
    assert_eq!(module.addr(), 0x1000);
    STARTUP.with_borrow_mut(|calls| calls.push("unload"));
}
unsafe extern "C" fn allocate(_: Log, _: Handle) -> Handle {
    STARTUP.with_borrow_mut(|calls| calls.push("allocate"));
    std::ptr::without_provenance_mut(0x2000)
}
unsafe extern "C" fn bind_required(native: Handle, _: &[Handle; startup::ENTRY_NAMES.len()]) {
    assert_eq!(native.addr(), 0x2000);
    STARTUP.with_borrow_mut(|calls| calls.push("bind"));
}
unsafe extern "C" fn fail_init(native: Handle, _: &InitPreferences) -> u32 {
    assert_eq!(native.addr(), 0x2000);
    STARTUP.with_borrow_mut(|calls| calls.push("init"));
    0
}
unsafe extern "C" fn borrow_device(_: Handle, _: Handle) {
    unreachable!()
}
unsafe extern "C" fn create_device(_: Handle, _: Handle) -> u32 {
    unreachable!()
}
unsafe extern "C" fn prime_mfg(_: Handle, _: u32) {
    unreachable!()
}
unsafe extern "C" fn unexpected_stage(_: Handle) -> u32 {
    unreachable!()
}
unsafe extern "C" fn queue_matches_device(_: Handle, _: Handle, _: &mut Handle) -> u32 {
    unreachable!()
}
unsafe extern "C" fn resolve_feature(_: Handle, _: u32, _: *const c_char, _: u32) -> u32 {
    unreachable!()
}
unsafe extern "C" fn dispose(native: Handle) {
    assert_eq!(native.addr(), 0x2000);
    STARTUP.with_borrow_mut(|calls| calls.push("dispose"));
}
unsafe extern "C" fn token(_: Handle, index: u32, out: &mut Handle) -> i32 {
    *out = std::ptr::without_provenance_mut(index as usize * 16);
    OK
}
unsafe extern "C" fn sleep_leaf(_: Handle, _: Handle) -> i32 {
    SLEEPS.set(SLEEPS.get() + 1);
    OK
}
unsafe extern "C" fn marker(_: Handle, _: Handle, marker: u32) -> i32 {
    MARKERS.with_borrow_mut(|markers| markers.push(marker));
    OK
}
unsafe extern "C" fn common(_: Handle, _: Handle, _: u32, _: *const c_void) -> i32 {
    OK
}
unsafe extern "C" fn reflex_state(_: Handle, _: u32, _: &[u64; 4], out: &mut Reflex) -> i32 {
    out.low_latency_available = 1;
    OK
}
unsafe extern "C" fn reflex_options(_: Handle, _: u32, _: u32) -> i32 {
    OK
}
unsafe extern "C" fn pcl_message(_: Handle, out: &mut u32) -> i32 {
    *out = PING;
    OK
}
unsafe extern "C" fn quiet(_: Handle) {}
unsafe extern "C" fn ring_capture(_: Handle) -> u32 {
    0
}
unsafe extern "C" fn ring_write(_: Handle, _: *const c_char, _: u64) {}
unsafe extern "C" fn generation_load(_: Handle, enabled: u32, _: u32, out: &mut u32) -> i32 {
    *out = if enabled != 0 { GENERATION } else { 0 };
    OK
}
unsafe extern "C" fn same_graphics(_: Handle, _: Handle, _: Handle) -> u32 {
    1
}
unsafe extern "C" fn support(_: Handle, _: Log, _: Handle) -> i32 {
    OK
}
unsafe extern "C" fn version(_: Handle, _: &mut Status) {}
unsafe extern "C" fn chain(_: Handle, _: &ChainDesc, _: &mut Handle) -> i32 {
    INIT_FAILED
}
unsafe extern "C" fn options(_: Handle, _: &Request) -> i32 {
    OK
}
unsafe extern "C" fn observe(_: Handle, _: &mut Observed) -> i32 {
    OK
}
unsafe extern "C" fn frame(_: Handle, _: *const Frame) -> i32 {
    OK
}
unsafe extern "C" fn command(_: Handle, _: Handle) -> i32 {
    OK
}
unsafe extern "C" fn constants(_: Handle, _: Handle, _: &Frame, _: u32, _: u32) -> i32 {
    OK
}
unsafe extern "C" fn tags(_: Handle, _: Handle, _: Handle, _: &Frame) -> i32 {
    OK
}

const STARTUP_PLATFORM: Startup = Startup {
    engine_count: 3,
    interposer_loaded,
    adapter_vendor,
    report_vendor,
    load,
    unload,
    allocate,
    bind_required,
    init: fail_init,
    borrow_device,
    create_device,
    prime_mfg,
    upgrade_device: unexpected_stage,
    native_device: unexpected_stage,
    start_audit: quiet,
    register_device: unexpected_stage,
    create_factory: unexpected_stage,
    upgrade_factory: unexpected_stage,
    queue_type: unexpected_stage,
    queue_matches_device,
    create_queue: unexpected_stage,
    native_queue: unexpected_stage,
    resolve_feature,
    prepare_mfg: quiet,
    pcl_defaults: quiet,
};

const PLATFORM: Platform = Platform {
    startup: &STARTUP_PLATFORM,
    dispose,
    token,
    sleep: sleep_leaf,
    marker,
    common,
    reflex_state,
    reflex_options,
    pcl_message,
    driver_sleep: quiet,
    ring_capture,
    ring_write,
    generation_load,
    same_graphics,
    support,
    version,
    chain,
    options,
    off: quiet,
    observe,
    frame,
    command,
    constants,
    tags,
};

fn host(profile: u32) -> Box<Host> {
    let mut setup: Setup = unsafe { zeroed() };
    setup.profile = profile;
    let mut host = Box::new(Host::new(&setup));
    host.native = NonNull::dangling().as_ptr();
    let entries = if profile == PROFILE_PCL {
        PCL_MARKER | PCL_STATE
    } else {
        GENERATION | REFLEX_OPTIONS | REFLEX_STATE | SLEEP | PCL_MARKER | PCL_STATE
    };
    host.entries = AtomicU32::new(entries);
    host.frames().pcl_message = PING;
    host
}

fn markers() -> Vec<u32> {
    MARKERS.with_borrow_mut(std::mem::take)
}

#[test]
fn failed_init_disposes_native_state_before_unloading_the_module() {
    let mut setup: Setup = unsafe { zeroed() };
    setup.struct_size = size_of::<Setup>() as u32;
    setup.abi_version = ABI_VERSION;
    setup.runtime_directory_utf8 = c"C:\\runtime".as_ptr();
    setup.dxgi_adapter = NonNull::<c_void>::dangling().as_ptr();
    setup.engine_version_utf8 = c"fixture".as_ptr();
    setup.project_id_utf8 = c"fixture".as_ptr();
    let mut host = null_mut();
    let mut looked_up = Vec::new();
    unsafe extern "system" fn stub_address() -> isize {
        0
    }
    let result = unsafe {
        create_host(
            &setup,
            [null_mut(); 2],
            &mut host,
            &PLATFORM,
            |module, name| {
                assert_eq!(module.as_ptr().addr(), 0x1000);
                looked_up.push(name.to_owned());
                Some(stub_address)
            },
        )
    };
    assert_eq!(result, Err(INIT_FAILED));
    assert!(host.is_null());
    assert!(
        looked_up
            .iter()
            .map(CString::as_c_str)
            .eq(startup::ENTRY_NAMES)
    );
    assert_eq!(
        STARTUP.with_borrow_mut(std::mem::take),
        ["load", "allocate", "bind", "init", "dispose", "unload"]
    );
}

#[test]
fn tokens_keep_source_order_and_retire_after_required_markers() {
    let host = host(PROFILE_DLSS_FG);
    assert_eq!(acquire(&host, 0, &PLATFORM), INVALID);
    assert_eq!(acquire(&host, 1, &PLATFORM), OK);
    assert_eq!(acquire(&host, 1, &PLATFORM), NOT_READY);
    assert_eq!(sleep(&host, 1, &PLATFORM), OK);
    assert_eq!(sleep(&host, 1, &PLATFORM), INVALID);
    assert_eq!(SLEEPS.get(), 1);
    assert_eq!(host.frames().tokens[0].index, 1);

    // Marker 6 without a controller is dropped; with one it maps to PCL marker 13.
    assert_eq!(marker_for(&host, 1, INPUT_SAMPLE, 0, &PLATFORM), OK);
    assert_eq!(marker_for(&host, 1, INPUT_SAMPLE, 1, &PLATFORM), OK);
    assert_eq!(marker_for(&host, 1, 0, 0, &PLATFORM), OK);
    assert_eq!(marker_for(&host, 1, 0, 0, &PLATFORM), INVALID);
    for marker in 1..=PRESENT_END {
        assert_eq!(marker_for(&host, 1, marker, 0, &PLATFORM), OK);
    }
    assert_eq!(markers(), [CONTROLLER_SAMPLE, 0, 1, 2, 3, 4, 5]);
    assert!(host.frames().slot(1).is_none());
    assert_eq!(marker_for(&host, 1, 0, 0, &PLATFORM), NOT_READY);
    assert_eq!(host.frames().marker_calls, 7);
}

#[test]
fn ring_refuses_a_live_slot_until_present_completion_retires_it() {
    let host = host(PROFILE_DLSS_FG);
    for id in 2..2 + TOKEN_SLOTS as u64 {
        assert_eq!(acquire(&host, id, &PLATFORM), OK);
    }
    let next = 2 + TOKEN_SLOTS as u64;
    assert_eq!(acquire(&host, next, &PLATFORM), NOT_READY);
    let raw = Box::into_raw(host);
    assert_eq!(unsafe { rsf_core_slhost_reset_sources(raw) }, NOT_READY);
    assert_eq!(unsafe { rsf_core_slhost_retire_through(raw, 2) }, OK);
    let host = unsafe { Box::from_raw(raw) };
    assert_eq!(acquire(&host, next, &PLATFORM), OK);
    // Token indices stay monotonic across the ring.
    assert_eq!(host.frames().tokens[0].index, TOKEN_SLOTS as u32 + 1);
    for id in 3..=next {
        assert_eq!(abort_for(&host, id), OK);
    }
    let raw = Box::into_raw(host);
    assert_eq!(unsafe { rsf_core_slhost_reset_sources(raw) }, OK);
    let host = unsafe { Box::from_raw(raw) };
    assert_eq!(acquire(&host, 1, &PLATFORM), OK);
}

#[test]
fn pcl_profile_mints_tokens_without_a_sleeper() {
    let host = host(PROFILE_PCL);
    let raw = Box::into_raw(host);
    assert_eq!(unsafe { rsf_core_slhost_begin(raw, 1, &PLATFORM) }, OK);
    assert_eq!(SLEEPS.get(), 0);
    assert_eq!(
        unsafe { rsf_core_slhost_reflex(raw, REFLEX_ON, 0, &PLATFORM) },
        NOT_SUPPORTED
    );
    let mut status: LatencyStatus = unsafe { zeroed() };
    status.struct_size = size_of::<LatencyStatus>() as u32;
    assert_eq!(
        unsafe { rsf_core_slhost_latency_status(raw, &mut status, &PLATFORM) },
        OK
    );
    assert!(status.pacing_owner == RSF_PACING_NONE && status.last_begin_id == 1);
    assert!(status.sleep_calls == 0 && status.pcl_message_id == PING);
    drop(unsafe { Box::from_raw(raw) });
}

#[test]
fn input_reports_controller_and_registered_ping_once_per_frame() {
    let host = host(PROFILE_DLSS_FG);
    assert_eq!(input_for(&host, 1, CONTROLLER, PING, &PLATFORM), NOT_READY);
    assert_eq!(acquire(&host, 1, &PLATFORM), OK);
    assert_eq!(input_for(&host, 1, 8, 0, &PLATFORM), INVALID);
    assert_eq!(input_for(&host, 1, CONTROLLER | 1, PING, &PLATFORM), OK);
    assert_eq!(input_for(&host, 1, CONTROLLER, PING, &PLATFORM), OK);
    assert_eq!(input_for(&host, 1, 2, PING + 1, &PLATFORM), OK);
    assert_eq!(markers(), [CONTROLLER_SAMPLE, PCL_PING]);
    assert_eq!(host.frames().input_counts, [1, 1, 2, 1]);
}

#[test]
fn reflex_options_are_recorded_and_common_constants_set_once() {
    let host = host(PROFILE_DLSS_FG);
    assert_eq!(reflex(&host, RSF_REFLEX_BOOST + 1, 0, &PLATFORM), INVALID);
    assert_eq!(reflex(&host, RSF_REFLEX_BOOST, 500, &PLATFORM), OK);
    let frames = host.frames();
    assert!(frames.reflex_mode == RSF_REFLEX_BOOST && frames.frame_limit_us == 500);
    drop(frames);
    assert_eq!(acquire(&host, 1, &PLATFORM), OK);
    let constants = std::ptr::null();
    assert!(common_for(&host, 1, 1, constants, &PLATFORM));
    assert!(!common_for(&host, 1, 1, constants, &PLATFORM));
    assert!(!common_for(&host, 1, 2, constants, &PLATFORM));
    assert!(common_for(&host, 1, 0, constants, &PLATFORM));
}

#[test]
fn generation_reload_waits_for_borrowers() {
    let host = host(PROFILE_DLSS_FG);
    host.borrowers.store(1, Relaxed);
    let raw = Box::into_raw(host);
    assert_eq!(
        unsafe { rsf_core_slhost_generation_load(raw, 0, &PLATFORM) },
        NOT_READY
    );
    let host = unsafe { Box::from_raw(raw) };
    host.borrowers.store(0, Relaxed);
    let raw = Box::into_raw(host);
    assert_eq!(
        unsafe { rsf_core_slhost_generation_load(raw, 0, &PLATFORM) },
        OK
    );
    let host = unsafe { Box::from_raw(raw) };
    assert!(!host.generation_loaded.load(Relaxed) && !host.has(GET_STATE));
    assert!(host.has(SLEEP | PCL_MARKER));
}
