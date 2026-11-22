// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::fake;
use crate::platform::fake::setup;
use std::ptr;

fn status(control: &Control, platform: &Platform) -> (c_int, Status) {
    let mut value = fake::status_header();
    // SAFETY: a complete local record.
    (unsafe { control.status(platform, &mut value) }, value)
}

#[test]
fn a_boot_in_progress_refuses_another_install() {
    let control = Control::new();
    let _reserved = control.boot_gate.lock().unwrap();
    assert_eq!(control.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert!(!control.booted());
}

#[test]
fn configure_advances_the_generation_only_for_sr_identity() {
    let control = Control::new();
    let mut settings = default_settings();
    settings.fg_mode = FG_FIXED;
    settings.latency_mode = 1;
    assert_eq!(control.configure(&fake::LEAVES, &settings), 1);
    assert_eq!(control.snapshot(), (settings, 1));
    assert_eq!(
        fake::FG_OPTIONS.get(),
        Some(FgOptions {
            mode: FG_FIXED,
            generated_frames: 1,
            reflex_mode: 1,
            frame_limit_us: 0,
            debug: 0,
        })
    );
    settings.quality = 2;
    assert_eq!(control.configure(&fake::LEAVES, &settings), 1);
    assert_eq!(control.snapshot().1, 2);
    settings.dlss_preset = 4;
    assert_eq!(control.configure(&fake::LEAVES, &settings), 1);
    assert_eq!(control.snapshot().1, 3);
}

#[test]
fn configure_refusals_keep_the_request() {
    let control = Control::new();
    let refused: [fn(&mut Settings); 9] = [
        |s| s.sr_backend = 8,
        |s| s.quality = 6,
        |s| s.dlss_preset = 16,
        |s| s.fg_mode = 2,
        |s| s.latency_mode = 3,
        |s| s.generated_frames = 0,
        |s| s.generated_frames = 16,
        |s| s.fg_backend = 2,
        |s| {
            s.sr_backend = SR_DLSS;
            s.quality = QUALITY_ULTRA_QUALITY;
        },
    ];
    for change in refused {
        let mut settings = default_settings();
        change(&mut settings);
        assert_eq!(control.configure(&fake::LEAVES, &settings), 0);
        assert_eq!(control.snapshot(), (default_settings(), 1));
    }
    let mut sharpening = default_settings();
    sharpening.sharpening = fake::sharpening_defaults();
    sharpening.sharpening.mode = 5;
    assert_eq!(control.configure(&fake::LEAVES, &sharpening), 0);
}

fn vulkan_booted() -> Control {
    fake::VK_FACTS.set(Default::default());
    let control = Control::new();
    fake::boot_vulkan(&control.vulkan, c"C:/game/runtime");
    fake::take_calls();
    control
}

#[test]
fn vulkan_receives_only_accepted_settings() {
    let control = vulkan_booted();
    let mut accepted = default_settings();
    accepted.quality = 2;
    assert_eq!(control.configure(&fake::LEAVES, &accepted), 1);
    assert_eq!(fake::take_calls(), ["facts", "latency_options"]);
    let mut refused = accepted;
    refused.generated_frames = 0;
    assert_eq!(control.configure(&fake::LEAVES, &refused), 0);
    assert!(fake::take_calls().is_empty());
    // The D3D12 request is untouched while Vulkan owns the session.
    assert_eq!(control.snapshot(), (default_settings(), 1));
    let mut output = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: SETTINGS_ABI_VERSION,
        ..Settings::default()
    };
    // SAFETY: a complete local record.
    assert_eq!(unsafe { control.settings(&mut output) }, 1);
    assert_eq!(output, accepted);
}

#[test]
fn boot_derives_the_session() {
    let control = fake::boot_d3d12();
    assert!(control.booted());
    assert_eq!(control.session_id(), 42);
    let paths = control.paths().expect("paths");
    // SAFETY: owner-held strings.
    let text = |value| {
        unsafe { CStr::from_ptr(value) }
            .to_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        text(paths.streamline_directory),
        "C:/game/runtime/streamline"
    );
    assert_eq!(
        text(paths.streamline_interposer),
        "C:/game/runtime/streamline/sl.interposer.dll"
    );
    assert_eq!(
        text(paths.fidelityfx_directory),
        "C:/game/runtime/fidelityfx"
    );
    assert_eq!(text(paths.xess_directory), "C:/game/runtime/xess");
    assert_eq!(text(paths.engine_version), "4.7.2");
    let installs = fake::INSTALLS.get();
    assert_eq!(control.boot(&fake::LEAVES, &setup(c"C:/other")), 1);
    assert_eq!(fake::INSTALLS.get(), installs);
}

#[test]
fn boot_refusals() {
    let control = Control::new();
    let mut missing = setup(c"C:/game/runtime");
    missing.project_id = ptr::null();
    assert_eq!(control.boot(&fake::LEAVES, &missing), 0);
    fake::INSTALL_RESULT.set(-3);
    assert_eq!(control.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert!(!control.booted());
    assert_eq!(status(&control, &fake::LEAVES).0, 0);
}

#[test]
fn d3d12_status_assembles_runtime_facts() {
    let control = fake::boot_d3d12();
    fake::FG.set(FgFacts {
        available: 1,
        backend: 3,
        switch_result: -3,
        last_result: -5,
        statistics_flags: 3,
        total_presented: 12,
        ..FgFacts::default()
    });
    let (result, value) = status(&control, &fake::LEAVES);
    assert_eq!(result, 1);
    assert_eq!(value.abi_version, ABI_VERSION);
    assert_eq!(value.session_id, 42);
    assert_eq!(
        (value.fg_backend, value.fg_available, value.fg_result),
        (3, 1, -3)
    );
    assert_eq!(
        (
            value.presentation_statistics_valid,
            value.presentation_statistics_flags
        ),
        (1, 3)
    );
    assert_eq!(
        (
            value.window_foreground,
            value.window_visible,
            value.foreground_process_id
        ),
        (1, 1, 10)
    );
    assert_eq!(value.keyboard_inputs, 0);
    fake::FG.set(FgFacts {
        last_result: -5,
        ..FgFacts::default()
    });
    fake::LATENCY.set(Some(LatencyFacts {
        available: 1,
        effective_mode: 2,
        paced_frames: 8,
        owner: 1,
        inputs: fake::INPUTS,
        ..LatencyFacts::default()
    }));
    let (_, value) = status(&control, &fake::LEAVES);
    assert_eq!(value.fg_result, -5);
    assert_eq!(
        (value.sleep_calls, value.latency_owner, value.mouse_inputs),
        (8, 1, 2)
    );
    assert_eq!((value.latency_available, value.latency_mode), (1, 2));
    assert_eq!(value.derived_boundaries, 7);
}

#[test]
fn vulkan_status_adds_inputs_and_window() {
    let control = vulkan_booted();
    let (result, value) = status(&control, &fake::LEAVES);
    assert_eq!(result, 1);
    assert_eq!(value.abi_version, ABI_VERSION);
    assert_eq!(value.session_id, 42);
    assert_eq!(
        (
            value.keyboard_inputs,
            value.pcl_pings,
            value.derived_boundaries
        ),
        (1, 4, 7)
    );
    assert_eq!(value.window_foreground, 1);
}

#[test]
fn sr_frames_report_the_effective_backend() {
    let control = fake::boot_d3d12();
    let frame = SrFrame {
        requested_sr: SR_SPATIAL,
        context_backend: 0,
        context_result: BACKEND_OK,
        evaluated: BACKEND_OK,
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
    };
    control.sr_frame(&frame);
    let (_, value) = status(&control, &fake::LEAVES);
    assert_eq!(
        (value.active_sr, value.requested_sr, value.evaluated_frames),
        (SR_SPATIAL, SR_SPATIAL, 1)
    );
    control.sr_frame(&SrFrame {
        requested_sr: SR_AUTO,
        context_backend: 3,
        context_result: -4,
        evaluated: -1,
        ..frame
    });
    let (_, value) = status(&control, &fake::LEAVES);
    assert_eq!(
        (value.active_sr, value.sr_result, value.refused_frames),
        (0, -4, 1)
    );
    control.sr_frame(&SrFrame {
        requested_sr: SR_AUTO,
        context_backend: 3,
        ..frame
    });
    assert_eq!(status(&control, &fake::LEAVES).1.active_sr, 3);
    control.sr_missed();
    assert_eq!(status(&control, &fake::LEAVES).1.active_sr, 0);
}

unsafe extern "C" fn engine_configure(_: *const Settings, viewport: u64) -> c_int {
    viewport as c_int
}

unsafe extern "C" fn engine_reset() {}

#[test]
fn engine_services_route_only_to_an_active_backend() {
    let services = EngineServices {
        struct_size: size_of::<EngineServices>() as u32,
        abi_version: ABI_VERSION,
        configure: Some(engine_configure),
        reset_history: Some(engine_reset),
    };
    let idle = Control::new();
    assert!(idle.attach_engine(&services));
    assert_eq!(idle.configure_engine(&default_settings(), 5), 0);
    let vulkan = vulkan_booted();
    assert!(vulkan.attach_engine(&services));
    assert_eq!(vulkan.configure_engine(&default_settings(), 5), 5);
    let control = fake::boot_d3d12();
    assert!(control.attach_engine(&services));
    assert_eq!(control.configure_engine(&default_settings(), 6), 6);
    assert!(!control.attach_engine(&EngineServices {
        reset_history: None,
        ..services
    }));
}

#[test]
fn exported_status_accepts_the_same_output_twice() {
    let mut output = fake::status_header();
    let setup = fake::setup(c"C:/game/runtime");
    // SAFETY: the fake table and the initialized setup live for the call; the output is writable.
    unsafe {
        assert_eq!(crate::rsf_godot_boot(&setup), 1);
        assert_eq!(crate::rsfg_status(&mut output), 1);
        assert_eq!(output.abi_version, ABI_VERSION);
        assert_eq!(crate::rsfg_status(&mut output), 1);
    }
}

#[test]
fn short_abi_records_need_only_the_size_word() {
    let short = size_of::<u32>() as u32;
    let size_word = &raw const short;
    // SAFETY: the size word is the entire advertised record.
    unsafe {
        assert_eq!(crate::rsfg_attach_engine(size_word.cast()), 0);
        assert_eq!(crate::rsf_godot_boot(size_word.cast()), 0);
        assert_eq!(crate::rsf_godot_configure(size_word.cast()), 0);
    }
}
