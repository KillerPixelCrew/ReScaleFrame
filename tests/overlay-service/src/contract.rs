// SPDX-License-Identifier: MIT
use super::{Checks, INVALID, Ini, OK, Query};
use rsf_game_sys::*;
use std::ffi::{CStr, c_void};
use std::os::windows::fs::OpenOptionsExt;
use std::ptr::{null, null_mut};

const ABI_MISMATCH: i32 = -2;
const NOT_READY: i32 = -3;
const BUSY: i32 = -4;

type Apply = unsafe extern "C" fn(*const rsf_render_settings) -> i32;
type Diagnostics = unsafe extern "C" fn(*mut rsf_overlay_diagnostics) -> i32;
type FgStatus = unsafe extern "C" fn(*mut rsf_native_fg_status) -> i32;
type Guarded = unsafe extern "C" fn(Apply) -> i32;

fn settings_header() -> rsf_render_settings {
    rsf_render_settings {
        struct_size: size_of::<rsf_render_settings>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    }
}

struct Callback {
    api: rsf_overlay_api,
    reenter: bool,
    changed: u32,
    last: rsf_render_settings,
    checks: Checks,
}

unsafe extern "C" fn changed(user: *mut c_void, applied: *const rsf_render_settings) {
    // The service invokes this synchronously on the fixture thread.
    let context = unsafe { &mut *user.cast::<Callback>() };
    let applied = unsafe { applied.read() };
    context.changed += 1;
    context.last = applied;
    if !std::mem::take(&mut context.reenter) {
        return;
    }
    let mut snapshot = settings_header();
    context.checks.check(
        unsafe { context.api.settings_get.unwrap()(&mut snapshot) } == OK
            && snapshot.sr_backend == applied.sr_backend
            && snapshot.fg_enabled == applied.fg_enabled,
        "settings_changed can read the newly published request",
    );
    let nested = rsf_render_settings {
        sr_backend: 1,
        fg_enabled: 0,
        ..applied
    };
    context.checks.check(
        unsafe { context.api.settings_apply.unwrap()(&nested) } == BUSY,
        "settings_changed receives BUSY for a nested write",
    );
}

unsafe extern "C" fn refuse_option(_: *mut c_void, _: u32, _: i32) -> i32 {
    0
}

fn table(query: Query, name_resolves: bool, checks: &mut Checks) -> Option<rsf_overlay_api> {
    // Use bytes for the sentinel table so no Rust function pointer carries the sentinel value.
    #[repr(C, align(8))]
    struct TableBytes([u8; size_of::<rsf_overlay_api>()]);
    let mut sentinel = TableBytes([0xa5; size_of::<rsf_overlay_api>()]);
    let length = size_of::<rsf_overlay_api>() as u32;
    sentinel.0[0..4].copy_from_slice(&(length - 1).to_ne_bytes());
    sentinel.0[4..8].copy_from_slice(&99u32.to_ne_bytes());
    let mut expected = sentinel.0;
    let ptr = sentinel.0.as_mut_ptr().cast();
    checks.check(
        unsafe { query(null_mut()) } == INVALID,
        "null table refused",
    );
    checks.check(
        unsafe { query(ptr) } == INVALID,
        "short table refused before version",
    );
    checks.check(sentinel.0 == expected, "short refusal wrote nothing");
    sentinel.0[0..4].copy_from_slice(&length.to_ne_bytes());
    expected[0..4].copy_from_slice(&length.to_ne_bytes());
    checks.check(
        unsafe { query(ptr) } == ABI_MISMATCH,
        "version mismatch refused",
    );
    checks.check(sentinel.0 == expected, "version refusal wrote nothing");
    checks.check(name_resolves, "entry point exported by name");
    let mut api = rsf_overlay_api {
        struct_size: length,
        abi_version: RSF_OVERLAY_API_VERSION,
        ..Default::default()
    };
    checks.check(
        unsafe { query(&mut api) } == OK
            && api.struct_size == length
            && api.abi_version == RSF_OVERLAY_API_VERSION,
        "table filled",
    );
    checks.check(
        api.configure.is_some()
            && api.present.is_some()
            && api.notify.is_some()
            && api.visible.is_some()
            && api.set_visible.is_some()
            && api.toggle.is_some()
            && api.capture.is_some()
            && api.status.is_some()
            && api.settings_get.is_some()
            && api.settings_apply.is_some(),
        "every entry populated",
    );
    (checks.failures == 0).then_some(api)
}

fn setup_refusals(api: &rsf_overlay_api, setup: rsf_overlay_service_setup, checks: &mut Checks) {
    let configure = api.configure.unwrap();
    let empty = [0u16];
    checks.check(unsafe { configure(null()) } == INVALID, "null setup");
    let invalid = [
        (
            rsf_overlay_service_setup {
                struct_size: setup.struct_size - 4,
                ..setup
            },
            INVALID,
            "short setup",
        ),
        (
            rsf_overlay_service_setup {
                abi_version: RSF_OVERLAY_API_VERSION + 1,
                ..setup
            },
            ABI_MISMATCH,
            "setup version",
        ),
        (
            rsf_overlay_service_setup {
                configuration_path: empty.as_ptr(),
                ..setup
            },
            INVALID,
            "empty path",
        ),
        (
            rsf_overlay_service_setup {
                configuration_section: null(),
                ..setup
            },
            INVALID,
            "missing section",
        ),
        (
            rsf_overlay_service_setup {
                sr_backend_choices: 1,
                ..setup
            },
            INVALID,
            "SR backend bit 0 is not a backend",
        ),
        (
            rsf_overlay_service_setup {
                sr_quality_choices: 0,
                ..setup
            },
            INVALID,
            "SR backends without any quality",
        ),
        (
            rsf_overlay_service_setup {
                sr_backend_choices: 0,
                defaults: rsf_render_settings {
                    sr_enabled: 1,
                    ..setup.defaults
                },
                ..setup
            },
            INVALID,
            "no SR but defaults enable SR",
        ),
        (
            rsf_overlay_service_setup {
                latency_choices: 1 << RSF_REFLEX_ON,
                ..setup
            },
            INVALID,
            "latency without Off",
        ),
        (
            rsf_overlay_service_setup {
                fg_backend_choices: 1 << 2,
                ..setup
            },
            INVALID,
            "unknown FG provider",
        ),
        (
            rsf_overlay_service_setup {
                defaults: rsf_render_settings {
                    sr_backend: 5,
                    ..setup.defaults
                },
                ..setup
            },
            INVALID,
            "defaults outside choices",
        ),
        (
            rsf_overlay_service_setup {
                defaults: rsf_render_settings {
                    abi_version: 1,
                    ..setup.defaults
                },
                ..setup
            },
            ABI_MISMATCH,
            "defaults version",
        ),
        (
            rsf_overlay_service_setup {
                option_count: RSF_OVERLAY_SERVICE_OPTION_LIMIT + 1,
                ..setup
            },
            INVALID,
            "too many adapter options",
        ),
        (
            rsf_overlay_service_setup {
                option_changed: None,
                ..setup
            },
            INVALID,
            "editable options require acknowledgement",
        ),
    ];
    for (request, result, message) in &invalid {
        checks.check(unsafe { configure(request) } == *result, message);
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_test_overlay_contract(
    query: Query,
    name_resolves: u32,
    guarded: Guarded,
    diagnostics_get: Diagnostics,
    fg_status_get: FgStatus,
) -> i32 {
    let mut checks = Checks::default();
    let Some(api) = table(query, name_resolves != 0, &mut checks) else {
        return 1;
    };
    let (get, apply, configure) = (
        api.settings_get.unwrap(),
        api.settings_apply.unwrap(),
        api.configure.unwrap(),
    );
    let (status_get, notify) = (api.status.unwrap(), api.notify.unwrap());
    let (visible, set_visible, toggle) = (
        api.visible.unwrap(),
        api.set_visible.unwrap(),
        api.toggle.unwrap(),
    );
    let mut settings = settings_header();
    let mut status = rsf_render_status {
        struct_size: size_of::<rsf_render_status>() as u32,
        abi_version: RSF_RENDER_FRONTEND_ABI_VERSION,
        ..Default::default()
    };
    let mut capture = rsf_overlay_capture {
        struct_size: size_of::<rsf_overlay_capture>() as u32,
        ..Default::default()
    };
    let get_capture = api.capture.unwrap();
    checks.check(
        unsafe { get(&mut settings) } == NOT_READY,
        "settings before configure",
    );
    checks.check(
        unsafe { status_get(&mut status) } == NOT_READY,
        "status before configure",
    );
    checks.check(
        unsafe { api.present.unwrap()(null_mut(), null_mut()) } == INVALID,
        "present null chain",
    );
    checks.check(
        unsafe { set_visible(1) == NOT_READY && toggle() == NOT_READY },
        "visibility before start",
    );
    checks.check(
        unsafe { notify(null_mut(), RSF_OVERLAY_NOTIFY_RESIZE) } == NOT_READY,
        "notify before configure",
    );
    checks.check(
        unsafe { get_capture(&mut capture) } == OK
            && capture.started == 0
            && capture.visible == 0
            && capture.captured == 0,
        "nothing captured before start",
    );
    capture.struct_size = 4;
    checks.check(
        unsafe { get_capture(&mut capture) } == INVALID,
        "short capture refused",
    );
    checks.check(unsafe { visible() } == 0, "not visible before start");

    let ini = Ini::new("rsf_overlay_dll_api", "Overlay");
    checks.check(
        [
            ("Foreign", "7"),
            ("OverlayLayout", "5"),
            ("SRBackend", "9"),
            ("SRQuality", "3"),
            ("Sharpness", "nan"),
            ("MotionScaleLimit", "-4"),
        ]
        .into_iter()
        .all(|(key, value)| ini.write(key, value)),
        "seed INI",
    );
    let mut callback = Callback {
        api,
        reenter: false,
        changed: 0,
        last: settings_header(),
        checks: Checks::default(),
    };
    let adapter_options = [rsf_overlay_service_option {
        label_utf8: c"Adapter option".as_ptr(),
        kind: RSF_OVERLAY_SERVICE_OPTION_TOGGLE,
        maximum: 1,
        step: 1,
        divisor: 1,
        ..Default::default()
    }; RSF_OVERLAY_SERVICE_OPTION_LIMIT as usize + 1];
    let setup = rsf_overlay_service_setup {
        struct_size: size_of::<rsf_overlay_service_setup>() as u32,
        abi_version: RSF_OVERLAY_API_VERSION,
        identity_utf8: c"contract fixture".as_ptr(),
        configuration_path: ini.wide.as_ptr(),
        configuration_section: ini.section.as_ptr(),
        defaults: rsf_render_settings {
            sr_backend: 1,
            fg_backend: RSF_FG_BACKEND_AUTO,
            generated_frames: 1,
            sr_quality: 2,
            latency: RSF_REFLEX_OFF,
            ..settings_header()
        },
        sr_backend_choices: (1 << 1) | (1 << 3),
        sr_quality_choices: 0x1f,
        fg_backend_choices: (1 << RSF_FG_BACKEND_FSR3) | (1 << RSF_FG_BACKEND_AUTO),
        latency_choices: (1 << RSF_REFLEX_OFF) | (1 << RSF_REFLEX_ON),
        max_generated_frames: 1,
        user: (&raw mut callback).cast(),
        settings_changed: Some(changed),
        options: adapter_options.as_ptr(),
        option_count: RSF_OVERLAY_SERVICE_OPTION_LIMIT,
        option_changed: Some(refuse_option),
        ..Default::default()
    };
    setup_refusals(&api, setup, &mut checks);
    checks.check(
        unsafe { get(&mut settings) } == NOT_READY,
        "refused setups did not configure",
    );
    checks.check(
        unsafe { configure(&setup) } == OK,
        "configure at the adapter option limit",
    );
    checks.check(unsafe { configure(&setup) } == BUSY, "second configure");
    checks.check(
        unsafe { get(&mut settings) } == OK,
        "settings after configure",
    );
    checks.check(
        settings.sr_backend == 1
            && settings.sr_quality == 3
            && settings.sr_enabled == 0
            && settings.fg_backend == RSF_FG_BACKEND_AUTO,
        "saved quality read and unsupported backend normalized",
    );
    checks.check(
        settings.sharpening_mode == 0
            && settings.sharpening_sharpness == 0.4
            && settings.sharpening_motion_scale_limit == 0.01,
        "configure loads and sanitizes sharpening",
    );
    checks.check(ini.read("SRBackend") == 9, "load does not rewrite the file");

    let invalid = [
        (
            rsf_render_settings {
                generated_frames: 2,
                ..settings
            },
            INVALID,
            "generated above adapter maximum",
        ),
        (
            rsf_render_settings {
                fg_enabled: 1,
                fg_backend: RSF_FG_BACKEND_DLSS,
                ..settings
            },
            INVALID,
            "FG provider outside choices",
        ),
        (
            rsf_render_settings {
                latency: RSF_REFLEX_BOOST,
                ..settings
            },
            INVALID,
            "latency outside choices",
        ),
        (
            rsf_render_settings {
                sr_backend: 4,
                ..settings
            },
            INVALID,
            "SR backend outside choices",
        ),
        (
            rsf_render_settings {
                abi_version: 1,
                ..settings
            },
            ABI_MISMATCH,
            "settings version",
        ),
    ];
    for (request, result, message) in &invalid {
        checks.check(unsafe { apply(request) } == *result, message);
    }
    checks.check(unsafe { apply(null()) } == INVALID, "null settings");
    checks.check(
        unsafe { guarded(apply) } == INVALID,
        "short settings at guard page refused without reading the tail",
    );
    checks.check(
        callback.changed == 0 && ini.read("FG") == 0xdead,
        "refusals changed nothing",
    );

    let request = rsf_render_settings {
        sr_enabled: 1,
        sr_backend: 3,
        sr_quality: 1,
        fg_enabled: 1,
        fg_backend: RSF_FG_BACKEND_FSR3,
        latency: RSF_REFLEX_ON,
        frame_limit_us: 8333,
        ..settings
    };
    callback.reenter = true;
    checks.check(
        unsafe { apply(&request) } == OK,
        "outer apply succeeds with reentrant callback",
    );
    checks.check(
        callback.changed == 1
            && callback.last.sr_backend == 3
            && callback.last.fg_backend == RSF_FG_BACKEND_FSR3,
        "adapter notified once",
    );
    checks.check(
        [
            ("SR", 1),
            ("SRBackend", 3),
            ("SRQuality", 1),
            ("FG", 1),
            ("FGBackend", RSF_FG_BACKEND_FSR3),
            ("Latency", RSF_REFLEX_ON),
            ("FrameLimitUs", 8333),
            ("Foreign", 7),
            ("OverlayLayout", 5),
        ]
        .into_iter()
        .all(|(key, value)| ini.read(key) == value),
        "settings persisted in adapter section with foreign keys",
    );
    let mut reread = settings_header();
    checks.check(
        unsafe { get(&mut reread) } == OK && reread.sr_backend == 3 && reread.fg_enabled == 1,
        "get reflects apply",
    );
    let mut fg = rsf_native_fg_status {
        struct_size: size_of::<rsf_native_fg_status>() as u32,
        ..Default::default()
    };
    checks.check(
        unsafe { fg_status_get(&mut fg) } != 0 && fg.requested.mode == RSF_FG_FIXED,
        "nested write leaves the outer FG request forwarded",
    );

    let sharpen = rsf_render_settings {
        sharpening_mode: 2,
        sharpening_sharpness: 0.75,
        sharpening_mas: 1,
        sharpening_motion_sharpness: -0.2,
        sharpening_motion_threshold: 3.0,
        sharpening_motion_scale_limit: 12.0,
        sharpening_contrast_enabled: 1,
        sharpening_contrast: 1.5,
        sharpening_depth_scale: 72.0,
        sharpening_depth_bias: 0.002,
        sharpening_clamp_output: 1,
        sharpening_debug: 1,
        ..reread
    };
    checks.check(
        unsafe { apply(&sharpen) } == OK,
        "sharpening applies and saves",
    );
    checks.check(
        unsafe { get(&mut reread) } == OK
            && reread.sharpening_sharpness == 0.75
            && reread.sharpening_mode == 2
            && ini.read("SharpeningMode") == 2
            && ini.read("MAS") == 1,
        "sharpening service snapshot and INI reflect request",
    );
    let invalid_sharpening = rsf_render_settings {
        sharpening_sharpness: 8.0,
        ..sharpen
    };
    checks.check(
        unsafe { apply(&invalid_sharpening) } == INVALID,
        "invalid sharpening refused",
    );

    #[repr(C)]
    struct Extended {
        value: rsf_render_settings,
        tail: u32,
    }
    let mut extended = Extended {
        value: rsf_render_settings {
            struct_size: size_of::<Extended>() as u32,
            ..sharpen
        },
        tail: 0xfeed_face,
    };
    checks.check(
        unsafe { apply(&extended.value) } == OK
            && callback.changed == 3
            && callback.last.struct_size == size_of::<rsf_render_settings>() as u32,
        "larger request accepted and callback receives the runtime size",
    );
    checks.check(
        unsafe { get(&mut extended.value) } == OK
            && extended.value.struct_size == size_of::<rsf_render_settings>() as u32
            && extended.tail == 0xfeed_face,
        "get writes only the known settings prefix",
    );

    let blocked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&ini.path);
    checks.check(blocked.is_ok(), "opened INI with writes denied");
    if let Ok(blocked) = blocked {
        let request = rsf_render_settings {
            fg_enabled: 0,
            sr_backend: 1,
            ..sharpen
        };
        checks.check(
            unsafe { apply(&request) } == NOT_READY,
            "save failure refused",
        );
        drop(blocked);
        checks.check(
            unsafe { get(&mut reread) } == OK
                && reread.sr_backend == 3
                && reread.fg_enabled == 1
                && callback.changed == 3
                && ini.read("SRBackend") == 3
                && ini.read("FG") == 1,
            "save failure retains the INI, in-memory settings and callback count",
        );
    }
    checks.check(
        unsafe { fg_status_get(&mut fg) } != 0
            && fg.requested.mode == RSF_FG_FIXED
            && fg.requested.generated_frames == 1
            && fg.requested.reflex_mode == RSF_REFLEX_ON
            && fg.requested.frame_limit_us == 8333,
        "FG options forwarded to presentation owner",
    );
    checks.check(unsafe { status_get(&mut status) } == OK, "status");
    checks.check(
        status.sr_enabled == 1
            && status.sr_requested_backend == 3
            && status.sr_effective_backend == 0
            && status.fg_requested == 1
            && status.fg_requested_backend == RSF_FG_BACKEND_FSR3
            && status.fg_effective_backend == 0
            && status.fg_active == 0
            && status.latency_requested == RSF_REFLEX_ON
            && status.latency_effective == RSF_REFLEX_OFF
            && status.sr_backend_choices == setup.sr_backend_choices,
        "requested and effective are separate",
    );
    checks.check(
        unsafe { CStr::from_ptr(status.reason.as_ptr()) } == c"Waiting for the first Present",
        "status reason before Present",
    );
    status.abi_version = 1;
    checks.check(
        unsafe { status_get(&mut status) } == ABI_MISMATCH,
        "status version",
    );
    checks.check(
        unsafe { notify(null_mut(), 99) } == INVALID,
        "unknown notify reason",
    );
    checks.check(
        unsafe {
            notify(null_mut(), RSF_OVERLAY_NOTIFY_RESIZE) == OK
                && notify(null_mut(), RSF_OVERLAY_NOTIFY_DEVICE_RESET) == OK
        },
        "resize and reset notifications",
    );
    checks.check(
        unsafe { set_visible(1) } == NOT_READY,
        "configured but not started",
    );
    let mut diagnostics = rsf_overlay_diagnostics {
        struct_size: size_of::<rsf_overlay_diagnostics>() as u32,
        ..Default::default()
    };
    checks.check(
        unsafe { diagnostics_get(&mut diagnostics) } == OK
            && diagnostics.started == 0
            && diagnostics.presents == 0
            && diagnostics.apply_attempts == 12
            && diagnostics.apply_refused == 9
            && diagnostics.last_apply_result == NOT_READY,
        "diagnostics count short, reentrant and failed writes",
    );
    if checks.failures != 0 || callback.checks.failures != 0 {
        return 1;
    }
    println!("PASS: overlay DLL API contract through the real Runtime import");
    0
}
