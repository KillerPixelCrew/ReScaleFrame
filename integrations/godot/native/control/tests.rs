// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::{ABI_VERSION, fake};
use std::ptr;

fn setup(runtime: &CStr) -> Setup {
    Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI_VERSION,
        runtime_directory: runtime.as_ptr(),
        engine_version: c"4.7.2".as_ptr(),
        project_id: c"125d2115-7176-46e1-8806-95b790f01717".as_ptr(),
        log: None,
        log_user: ptr::null_mut(),
    }
}

fn booted() -> Control {
    let control = Control::new();
    fake::INSTALL_RESULT.set(BACKEND_OK);
    assert_eq!(control.boot(&fake::D3D12, &setup(c"C:/game/runtime")), 1);
    control
}

fn status(control: &Control, platform: &Platform) -> (c_int, Status) {
    let mut value = Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: ABI_VERSION,
        ..Status::default()
    };
    // SAFETY: a complete local record.
    (unsafe { control.status(platform, &mut value) }, value)
}

#[test]
fn configure_advances_the_generation_only_for_sr_identity() {
    let control = Control::new();
    let mut settings = default_settings();
    settings.fg_mode = FG_FIXED;
    settings.latency_mode = 1;
    assert_eq!(control.configure(&fake::D3D12, &settings), 1);
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
    assert_eq!(control.configure(&fake::D3D12, &settings), 1);
    assert_eq!(control.snapshot().1, 2);
    settings.dlss_preset = 4;
    assert_eq!(control.configure(&fake::D3D12, &settings), 1);
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
        assert_eq!(control.configure(&fake::D3D12, &settings), 0);
        assert_eq!(control.snapshot(), (default_settings(), 1));
    }
    let mut sharpening = default_settings();
    sharpening.sharpening = fake::sharpening_defaults();
    sharpening.sharpening.mode = 5;
    assert_eq!(control.configure(&fake::D3D12, &sharpening), 0);
}

#[test]
fn vulkan_receives_only_accepted_settings() {
    let control = Control::new();
    assert_eq!(
        control.configure(&fake::VULKAN, &default_settings()),
        fake::VULKAN_CONFIGURED
    );
    let mut refused = default_settings();
    refused.generated_frames = 0;
    assert_eq!(control.configure(&fake::VULKAN, &refused), 0);
    assert_eq!(control.snapshot().1, 1);
}

#[test]
fn boot_derives_the_session() {
    let control = booted();
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
    assert_eq!(control.boot(&fake::D3D12, &setup(c"C:/other")), 1);
    assert_eq!(fake::INSTALLS.get(), installs);
}

#[test]
fn boot_refusals() {
    let control = Control::new();
    let mut missing = setup(c"C:/game/runtime");
    missing.project_id = ptr::null();
    assert_eq!(control.boot(&fake::D3D12, &missing), 0);
    fake::INSTALL_RESULT.set(-3);
    assert_eq!(control.boot(&fake::D3D12, &setup(c"C:/game/runtime")), 0);
    assert!(!control.booted());
    assert_eq!(status(&control, &fake::D3D12).0, 0);
}

#[test]
fn d3d12_status_assembles_runtime_facts() {
    let control = booted();
    fake::FG.set(FgFacts {
        available: 1,
        backend: 3,
        switch_result: -3,
        last_result: -5,
        statistics_flags: 3,
        total_presented: 12,
        ..FgFacts::default()
    });
    let (result, value) = status(&control, &fake::D3D12);
    assert_eq!(result, 1);
    assert_eq!(value.abi_version, D3D12_STATUS_RECORD_ABI);
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
        paced_frames: 8,
        owner: 1,
        inputs: fake::INPUTS,
        ..LatencyFacts::default()
    }));
    let (_, value) = status(&control, &fake::D3D12);
    assert_eq!(value.fg_result, -5);
    assert_eq!(
        (value.sleep_calls, value.latency_owner, value.mouse_inputs),
        (8, 1, 2)
    );
    assert_eq!(value.derived_boundaries, 7);
}

#[test]
fn vulkan_status_adds_inputs_and_window() {
    let control = Control::new();
    let (result, value) = status(&control, &fake::VULKAN);
    assert_eq!(result, 1);
    assert_eq!(value.session_id, fake::VULKAN_SESSION);
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
    let control = booted();
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
    let (_, value) = status(&control, &fake::D3D12);
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
    let (_, value) = status(&control, &fake::D3D12);
    assert_eq!(
        (value.active_sr, value.sr_result, value.refused_frames),
        (0, -4, 1)
    );
    control.sr_frame(&SrFrame {
        requested_sr: SR_AUTO,
        context_backend: 3,
        ..frame
    });
    assert_eq!(status(&control, &fake::D3D12).1.active_sr, 3);
    control.sr_missed();
    assert_eq!(status(&control, &fake::D3D12).1.active_sr, 0);
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
    assert_eq!(idle.configure_engine(&fake::D3D12, ptr::null(), 5), 0);
    assert_eq!(idle.configure_engine(&fake::VULKAN, ptr::null(), 5), 5);
    let control = booted();
    assert!(control.attach_engine(&services));
    assert_eq!(control.configure_engine(&fake::D3D12, ptr::null(), 6), 6);
    assert!(!control.attach_engine(&EngineServices {
        reset_history: None,
        ..services
    }));
}
