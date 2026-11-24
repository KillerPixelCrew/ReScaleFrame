// SPDX-License-Identifier: MIT
use super::*;
use crate::fg_stage::Frontend;
use crate::sr_stage::OverlayChoices;
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, AtomicUsize};

fn lines(log: &Mutex<Vec<String>>) -> Vec<String> {
    lock(log).clone()
}

unsafe extern "C" fn collect(user: *mut c_void, message: *const c_char) {
    let log = unsafe { &*user.cast::<Mutex<Vec<String>>>() };
    lock(log).push(text(message));
}

fn log_into(log: &Mutex<Vec<String>>) -> Log {
    Log {
        callback: Some(collect),
        user: ptr::from_ref(log) as usize,
    }
}

fn reason_of(refusal: Refusal) -> String {
    refusal.text()
}

// ---- fake Runtime table ----

unsafe extern "C" fn ok_configure(_: *const rsf_overlay_service_setup) -> i32 {
    OK
}
unsafe extern "C" fn ok_present(_: *mut c_void, drawn: *mut u32) -> i32 {
    unsafe { drawn.write(1) };
    OK
}
unsafe extern "C" fn ok_notify(_: *mut c_void, _: u32) -> i32 {
    OK
}
unsafe extern "C" fn hidden() -> u32 {
    0
}
unsafe extern "C" fn ok_set_visible(_: u32) -> i32 {
    OK
}
unsafe extern "C" fn ok_toggle() -> i32 {
    OK
}
unsafe extern "C" fn ok_capture(capture: *mut rsf_overlay_capture) -> i32 {
    unsafe { (*capture).captured = RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD };
    OK
}
unsafe extern "C" fn ok_status(_: *mut rsf_render_status) -> i32 {
    OK
}
unsafe extern "C" fn ok_settings_get(_: *mut rsf_render_settings) -> i32 {
    OK
}
unsafe extern "C" fn ok_settings_apply(_: *const rsf_render_settings) -> i32 {
    OK
}

fn table() -> rsf_overlay_api {
    rsf_overlay_api {
        struct_size: size_of::<rsf_overlay_api>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        configure: Some(ok_configure),
        present: Some(ok_present),
        notify: Some(ok_notify),
        visible: Some(hidden),
        set_visible: Some(ok_set_visible),
        toggle: Some(ok_toggle),
        capture: Some(ok_capture),
        status: Some(ok_status),
        settings_get: Some(ok_settings_get),
        settings_apply: Some(ok_settings_apply),
    }
}

unsafe extern "C" fn mismatched_query(_: *mut rsf_overlay_api) -> i32 {
    ABI_MISMATCH
}
unsafe extern "C" fn short_query(out: *mut rsf_overlay_api) -> i32 {
    unsafe {
        out.write(rsf_overlay_api {
            struct_size: 8,
            ..table()
        })
    };
    OK
}
unsafe extern "C" fn good_query(out: *mut rsf_overlay_api) -> i32 {
    unsafe { out.write(table()) };
    OK
}

#[test]
fn binding_refuses_cleanly_and_accepts_a_complete_table() {
    let missing = bind(None).err().unwrap();
    assert_eq!(missing.status, API_MISSING);
    let mismatch = bind(Some(mismatched_query)).err().unwrap();
    assert_eq!(mismatch.status, API_REFUSED);
    assert!(reason_of(mismatch).contains("version"));
    assert!(reason_of(bind(Some(short_query)).err().unwrap()).contains("short"));
    for strip in 0..10 {
        let mut incomplete = table();
        match strip {
            0 => incomplete.configure = None,
            1 => incomplete.present = None,
            2 => incomplete.notify = None,
            3 => incomplete.visible = None,
            4 => incomplete.set_visible = None,
            5 => incomplete.toggle = None,
            6 => incomplete.capture = None,
            7 => incomplete.status = None,
            8 => incomplete.settings_get = None,
            _ => incomplete.settings_apply = None,
        }
        assert!(reason_of(validate(OK, &incomplete).err().unwrap()).contains("missing"));
    }
    let mut other = table();
    other.abi_version += 1;
    assert_eq!(validate(OK, &other).err().unwrap().status, API_REFUSED);
    assert!(bind(Some(good_query)).is_ok());

    let mut out = rsf_overlay_api {
        struct_size: 0x1234,
        ..Default::default()
    };
    let mut reason = ptr::null();
    let status = unsafe { rsf_anomaly_overlay_bind(Some(mismatched_query), &mut out, &mut reason) };
    assert_eq!((status, out.struct_size), (API_REFUSED, 0x1234));
    assert!(!reason.is_null());
    assert_eq!(
        unsafe { rsf_anomaly_overlay_bind(Some(good_query), &mut out, &mut reason) },
        STARTED
    );
    assert_eq!(out.abi_version, RSF_OVERLAY_API_VERSION);
}

// ---- fake leaves ----

static SR_OFFER: AtomicU32 = AtomicU32::new(0);
static FG_LINK: AtomicU32 = AtomicU32::new(0);
static SR_REQUESTS: AtomicU32 = AtomicU32::new(0);
static DEVICE: AtomicUsize = AtomicUsize::new(0x1000);
static CAPTURES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static INSERT: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The Present hook install refuses, as a dummy swap chain that cannot be created does.
    static PRESENT_REFUSED: Cell<bool> = const { Cell::new(false) };
}

/// The same-named stand-ins `overlay` calls under test (`native` in production).
pub(super) mod leaves {
    use super::*;

    pub(in crate::overlay) fn sr_choices(_: *mut c_void) -> OverlayChoices {
        if SR_OFFER.load(Ordering::SeqCst) == 0 {
            return Default::default();
        }
        OverlayChoices {
            backends: (1 << 1) | (1 << 5),
            qualities: 0x1f,
            preset: true,
            defaults: rsf_anomaly_sr_settings {
                enabled: true,
                backend: 5,
                quality: 2,
                preset: 4,
            },
        }
    }
    pub(in crate::overlay) fn sr_report(_: *mut rsf_overlay_sr_report) -> bool {
        false
    }
    pub(in crate::overlay) fn sr_request(_: &rsf_anomaly_sr_settings) {
        SR_REQUESTS.fetch_add(1, Ordering::SeqCst);
    }
    pub(in crate::overlay) fn fg_options() -> Option<Frontend> {
        (FG_LINK.load(Ordering::SeqCst) != 0).then_some(Frontend {
            enabled: 1,
            backend: 3,
            generated: 2,
            latency: 1,
            frame_limit_us: 7000,
        })
    }
    pub(in crate::overlay) fn fg_foreign_chain(_: *mut c_void) -> bool {
        false
    }
    pub(in crate::overlay) unsafe extern "C" fn chain(
        _: *mut c_void,
        out: *mut rsf_anomaly_overlay_chain,
    ) -> u32 {
        unsafe {
            out.write(rsf_anomaly_overlay_chain {
                window: ptr::without_provenance_mut(0x20),
                device: ptr::without_provenance(DEVICE.load(Ordering::SeqCst)),
                width: 1920,
                height: 1080,
            })
        };
        1
    }
    pub(in crate::overlay) unsafe extern "C" fn capture_backbuffer(
        _: *mut c_void,
        dump: rsf_input_dump_named_texture_fn,
        request: *const rsf_named_frame_dump,
        report: *mut rsf_named_frame_dump_report,
    ) -> i32 {
        assert!(dump.is_some());
        let request = unsafe { &*request };
        assert_eq!(request.abi_version, RSF_NAMED_FRAME_DUMP_ABI_VERSION);
        lock(&CAPTURES).push(text(request.name));
        unsafe {
            (*report).width = 1920;
            (*report).height = 1080;
        }
        OK
    }
    pub(in crate::overlay) fn install_input(_: &HookOps, _: &mut *const c_char) -> bool {
        true
    }
    pub(in crate::overlay) fn install_present(_: &HookOps, reason: &mut *const c_char) -> bool {
        if PRESENT_REFUSED.get() {
            *reason = c"dummy swap chain creation failed".as_ptr();
            return false;
        }
        true
    }
    pub(in crate::overlay) fn engine_window(_: *mut c_void) -> bool {
        true
    }
    pub(in crate::overlay) fn insert_held(_: *mut c_void) -> bool {
        INSERT.load(Ordering::SeqCst)
    }
}

unsafe extern "C" fn dump(
    _: *mut c_void,
    _: *mut c_void,
    _: *const rsf_named_frame_dump,
    _: *mut rsf_named_frame_dump_report,
) -> i32 {
    OK
}

const UNUSED_HOOKS: HookOps = HookOps {
    initialize: unused_initialize,
    create: unused_create,
    remove_unqueued: unused_remove,
    queue: unused_queue,
    apply: unused_initialize,
};
unsafe extern "C" fn unused_initialize() -> bool {
    unreachable!()
}
unsafe extern "C" fn unused_create(_: *mut c_void, _: *mut c_void, _: *mut *mut c_void) -> bool {
    unreachable!()
}
unsafe extern "C" fn unused_remove(_: *mut c_void) {
    unreachable!()
}
unsafe extern "C" fn unused_queue(_: *mut c_void, _: bool) -> bool {
    unreachable!()
}

const INI: &[u16] = &[
    b'C' as u16,
    b':' as u16,
    b'\\' as u16,
    b'r' as u16,
    b's' as u16,
    b'f' as u16,
    b'-' as u16,
    b'n' as u16,
    b'o' as u16,
    b'n' as u16,
    b'e' as u16,
    b'\\' as u16,
    b'a' as u16,
    b'.' as u16,
    b'i' as u16,
    b'n' as u16,
    b'i' as u16,
    0,
];

fn options(api: &rsf_overlay_api, install_hooks: bool) -> rsf_anomaly_overlay_options {
    rsf_anomaly_overlay_options {
        runtime: ptr::null_mut(),
        configuration: INI.as_ptr(),
        log: None,
        log_user: ptr::null_mut(),
        api_override: api,
        install_hooks: install_hooks.into(),
    }
}

unsafe extern "C" fn option_value() -> i32 {
    1
}
unsafe extern "C" fn option_set(value: i32) -> bool {
    value == 1
}

#[test]
fn setup_translates_what_the_adapter_can_apply() {
    let api = table();
    let start = options(&api, false);
    let registered = [rsf_anomaly_overlay_option {
        label: c"Tree and grass motion".as_ptr(),
        toggle: 1,
        restart: 0,
        minimum: 0,
        maximum: 1,
        step: 1,
        divisor: 1,
        get: Some(option_value),
        set: Some(option_set),
    }];
    let adapter: Vec<_> = registered.iter().filter_map(AdapterOption::new).collect();
    let mut incomplete = registered[0];
    incomplete.set = None;
    assert!(AdapterOption::new(&incomplete).is_none());
    assert!(!adapter[0].apply(0));
    assert!(adapter[0].apply(1));
    assert!(!adapter[0].apply(2));

    let mut table = Vec::new();
    // Neither SR Stage 5 nor the FG bridge: SR unavailable, FG Off only.
    let setup = make_setup(&start, INI.as_ptr(), &adapter, &mut table);
    assert_eq!(setup.abi_version, RSF_OVERLAY_API_VERSION);
    assert_eq!(
        setup.struct_size as usize,
        size_of::<rsf_overlay_service_setup>()
    );
    assert_eq!(unsafe { CStr::from_ptr(setup.identity_utf8) }, c"Anomaly");
    assert_eq!(
        unsafe { crate::ini::path(setup.configuration_path) },
        Some(INI)
    );
    assert_eq!(
        unsafe { crate::ini::path(setup.configuration_section) },
        Some("Anomaly\0".encode_utf16().collect::<Vec<_>>().as_slice())
    );
    assert_eq!((setup.sr_backend_choices, setup.sr_quality_choices), (0, 0));
    assert!(setup.sr_report.is_none());
    assert_eq!(setup.defaults.sr_backend, RSF_SR_NONE);
    assert_eq!(
        (setup.defaults.sr_enabled, setup.defaults.fg_enabled),
        (0, 0)
    );
    assert_eq!(setup.defaults.fg_backend, RSF_FG_BACKEND_AUTO);
    assert_eq!(setup.fg_backend_choices, 0);
    assert_eq!(setup.latency_choices, 1 << RSF_REFLEX_OFF);
    assert_eq!(setup.max_generated_frames, 1);
    assert_eq!(setup.dlss_preset_available, 0);
    assert!(setup.accept_window.is_some() && setup.settings_changed.is_some());
    assert_eq!(setup.option_count, 1);
    let described = unsafe { &*setup.options };
    assert_eq!(
        (described.kind, described.value, described.restart),
        (RSF_OVERLAY_SERVICE_OPTION_TOGGLE, 1, 0)
    );

    FG_LINK.store(1, Ordering::SeqCst);
    let fg_only = make_setup(&start, INI.as_ptr(), &[], &mut table);
    assert_eq!(fg_only.sr_backend_choices, 0);
    assert_ne!(fg_only.fg_backend_choices, 0);
    assert_eq!(fg_only.defaults.fg_enabled, 1);

    SR_OFFER.store(1, Ordering::SeqCst);
    let setup = make_setup(&start, INI.as_ptr(), &[], &mut table);
    SR_OFFER.store(0, Ordering::SeqCst);
    FG_LINK.store(0, Ordering::SeqCst);
    assert_eq!(
        (setup.sr_backend_choices, setup.sr_quality_choices),
        ((1 << 1) | (1 << 5), 0x1f)
    );
    assert!(setup.sr_report.is_some());
    assert_eq!(
        (
            setup.defaults.sr_enabled,
            setup.defaults.sr_backend,
            setup.defaults.sr_quality,
            setup.defaults.dlss_preset
        ),
        (1, 5, 2, 4)
    );
    assert_eq!(setup.dlss_preset_available, 1);
    assert_eq!(
        setup.fg_backend_choices,
        RSF_FG_BACKEND_ALL | (1 << RSF_FG_BACKEND_AUTO)
    );
    assert_eq!(setup.latency_choices, 0b111);
    assert_eq!(setup.max_generated_frames, 3);
    assert_eq!(
        (
            setup.defaults.fg_enabled,
            setup.defaults.fg_backend,
            setup.defaults.generated_frames,
            setup.defaults.latency,
            setup.defaults.frame_limit_us
        ),
        (1, 3, 2, 1, 7000)
    );
    assert!(setup.options.is_null() && setup.option_count == 0);
}

static CONFIGURES: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn refused_configure(_: *const rsf_overlay_service_setup) -> i32 {
    -1
}

unsafe extern "C" fn captured(out: *mut rsf_overlay_capture) -> i32 {
    unsafe { (*out).captured = RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD };
    OK
}

unsafe extern "C" fn refused_capture(out: *mut rsf_overlay_capture) -> i32 {
    unsafe { captured(out) };
    -1
}

#[test]
fn runtime_start_and_capture_report_refusals() {
    let log = Mutex::new(Vec::new());
    let placeholder = table();
    let mut start = options(&placeholder, false);
    start.api_override = ptr::null();
    start.log = Some(collect);
    start.log_user = ptr::from_ref(&log).cast_mut().cast();
    let overlay = Overlay::new();
    assert_eq!(
        overlay.start(&start, None, Vec::new()).unwrap_err().status,
        API_MISSING
    );
    assert!(
        lines(&log)
            .iter()
            .any(|line| line.contains("does not export"))
    );

    let api = rsf_overlay_api {
        configure: Some(refused_configure),
        ..table()
    };
    start.api_override = &api;
    assert_eq!(
        overlay.start(&start, None, Vec::new()).unwrap_err().status,
        CONFIGURE_REFUSED
    );
    assert!(lines(&log).iter().any(|line| line.contains("rejected")));

    for (capture, expected) in [
        (
            captured as unsafe extern "C" fn(*mut rsf_overlay_capture) -> i32,
            3,
        ),
        (refused_capture, 0),
    ] {
        let api = rsf_overlay_api {
            capture: Some(capture),
            ..table()
        };
        assert_eq!(service(&api, Log::default(), None).capture_bits(), expected);
    }
}

unsafe extern "C" fn counted_configure(_: *const rsf_overlay_service_setup) -> i32 {
    CONFIGURES.fetch_add(1, Ordering::SeqCst);
    OK
}

// The C++ service answered a start after a hook refusal with already_started, which the cold entry accepts as a
// started overlay. Configure is one-shot and the guarded hook installs do not repeat, so the retry must stay refused.
#[test]
fn a_hook_refusal_stays_refused_on_retry() {
    let api = rsf_overlay_api {
        configure: Some(counted_configure),
        ..table()
    };
    PRESENT_REFUSED.set(true);
    let overlay = Overlay::new();
    let first = overlay
        .start(&options(&api, true), Some(&UNUSED_HOOKS), Vec::new())
        .unwrap_err();
    assert_eq!(first.status, HOOKS_REFUSED);
    assert_eq!(reason_of(first), "dummy swap chain creation failed");
    let retry = overlay
        .start(&options(&api, true), Some(&UNUSED_HOOKS), Vec::new())
        .unwrap_err();
    assert_eq!(retry.status, HOOKS_REFUSED);
    assert!(reason_of(retry).contains("refused earlier"));
    assert_eq!(CONFIGURES.load(Ordering::SeqCst), 1);
    assert!(overlay.running().is_none());
    PRESENT_REFUSED.set(false);

    // Without hooks the same table starts once; the next start reports it.
    let overlay = Overlay::new();
    let mut empty = options(&api, false);
    empty.configuration = w!("");
    assert_eq!(
        overlay.start(&empty, None, Vec::new()).unwrap_err().status,
        CONFIGURE_REFUSED
    );
    assert_eq!(
        overlay.start(&options(&api, false), None, Vec::new()),
        Ok(STARTED)
    );
    assert_eq!(
        overlay
            .start(&options(&api, false), None, Vec::new())
            .unwrap_err()
            .status,
        ALREADY_STARTED
    );
    assert_eq!(CONFIGURES.load(Ordering::SeqCst), 2);
    assert!(overlay.running().is_some());
}

impl PartialEq for Refusal {
    fn eq(&self, other: &Self) -> bool {
        self.status == other.status && self.text() == other.text()
    }
}

// ---- the Present path ----

static VISIBLE: AtomicU32 = AtomicU32::new(0);
static RESETS: AtomicU32 = AtomicU32::new(0);
static REFUSE_RESET: AtomicBool = AtomicBool::new(false);
const NOT_READY: i32 = -3;
static TOGGLES: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn visible() -> u32 {
    VISIBLE.load(Ordering::SeqCst)
}
unsafe extern "C" fn count_notify(_: *mut c_void, reason: u32) -> i32 {
    if reason == RSF_OVERLAY_NOTIFY_DEVICE_RESET {
        RESETS.fetch_add(1, Ordering::SeqCst);
        if REFUSE_RESET.load(Ordering::SeqCst) {
            return NOT_READY;
        }
    }
    OK
}
unsafe extern "C" fn count_toggle() -> i32 {
    TOGGLES.fetch_add(1, Ordering::SeqCst);
    OK
}
fn service(api: &rsf_overlay_api, log: Log, capture: Option<Capture>) -> Service {
    Service {
        api: Api::new(api).unwrap(),
        log,
        diagnostics: None,
        capture,
        key_poll: true,
        options: Vec::new(),
    }
}

#[test]
fn the_panel_restarts_on_a_new_chain_and_polls_insert() {
    let log = Mutex::new(Vec::new());
    let api = rsf_overlay_api {
        notify: Some(count_notify),
        toggle: Some(count_toggle),
        ..table()
    };
    let service = service(&api, log_into(&log), None);
    let mut panel = Panel::default();
    let first = ptr::without_provenance_mut(0x100);
    service.present(&mut panel, first, false);
    service.present(&mut panel, first, false);
    assert_eq!(RESETS.load(Ordering::SeqCst), 0);
    let next = ptr::without_provenance_mut(0x200);
    REFUSE_RESET.store(true, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    assert_eq!(RESETS.load(Ordering::SeqCst), 1);
    assert_eq!(panel.chain, first.addr());
    REFUSE_RESET.store(false, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    assert_eq!(RESETS.load(Ordering::SeqCst), 2);
    assert_eq!(panel.chain, next.addr());
    DEVICE.store(0x2000, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    assert_eq!(RESETS.load(Ordering::SeqCst), 3);
    assert!(
        lines(&log)
            .iter()
            .all(|line| line.contains("restarts on it"))
    );

    INSERT.store(true, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    service.present(&mut panel, next, false);
    INSERT.store(false, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    INSERT.store(true, Ordering::SeqCst);
    service.present(&mut panel, next, false);
    assert_eq!(TOGGLES.load(Ordering::SeqCst), 2);

    let overlay = Overlay::new();
    assert!(overlay.service.set(service).is_ok());
    overlay.started.store(true, Ordering::Release);
    let panel = Mutex::new(panel);
    assert!(overlay.suspend(&panel));
    assert!(overlay.running().is_none());
    assert_eq!(RESETS.load(Ordering::SeqCst), 3);
    let mut panel = lock(&panel);
    assert!(panel.reset_pending);
    let service = overlay.committed().unwrap();
    service.present(&mut panel, next, true);
    assert_eq!(RESETS.load(Ordering::SeqCst), 4);
    assert!(!panel.reset_pending && panel.chain == 0);
    assert_eq!(TOGGLES.load(Ordering::SeqCst), 2);
    overlay.suspended.store(false, Ordering::Release);
    assert!(overlay.running().is_some());
}

#[test]
fn panel_edges_log_and_capture_the_configured_frames() {
    let log = Mutex::new(Vec::new());
    let api = rsf_overlay_api {
        visible: Some(visible),
        ..table()
    };
    let capture = Capture {
        frames: 2,
        stride: 3,
        cycles: 1,
        directory: c"C:\\captures".to_owned(),
        dump,
    };
    let service = service(&api, log_into(&log), Some(capture));
    let mut run = PanelRun::default();
    let chain = ptr::without_provenance_mut(0x100);
    for cycle in 0..2 {
        for visible in [1, 0] {
            VISIBLE.store(visible, Ordering::SeqCst);
            for frame in 0..10 {
                service.track(
                    &mut run,
                    chain,
                    [64, 32],
                    if frame == 1 { -9 } else { OK },
                    1,
                );
            }
        }
        assert_eq!((run.opens, run.closes), (cycle + 1, cycle + 1));
    }
    assert_eq!(
        *lock(&CAPTURES),
        ["open01_f00", "open01_f01", "close01_f00", "close01_f01"]
    );
    let lines = lines(&log);
    assert!(lines[0].starts_with("Anomaly overlay panel #1 opened: target 64x32"));
    assert!(lines.iter().any(|line| line
        == "Anomaly overlay panel #1 frame 2: api.present result -9, drawn 1, target 64x32."));
    assert!(lines.iter().any(|line| line.starts_with(
        "Anomaly overlay panel #1 closed: present drawn 10 of 10 visible frames (1 refused, last result 0); input and settings counters unavailable."
    )));
    assert!(lines.iter().any(|line| line.starts_with(
        "Anomaly overlay capture close01_f01: result 0 (1920x1080 format 0) -> C:\\captures"
    )));
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("Anomaly overlay panel #2 opened"))
    );
}
