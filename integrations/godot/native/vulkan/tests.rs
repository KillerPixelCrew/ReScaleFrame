// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::fake;
use std::ffi::CStr;
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

const WINDOW: usize = 0x1234;

/// A booted session whose engine reported its primary window.
fn booted() -> Vulkan {
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    vulkan.report_window(ptr::without_provenance_mut(WINDOW));
    fake::take_calls();
    vulkan
}

/// Every device, provider and presenter fact present.
fn ready() -> VkFacts {
    VkFacts {
        xess_device_ready: 1,
        interop_device_ready: 1,
        graph_handoff: 1,
        fsr4_sr_compiled: 1,
        fsr4_fg_compiled: 1,
        xess_fg_compiled: 1,
        presenter: 1,
        reserved: 0,
        presenter_backend: FG_FSR3,
        sdk_shutdown_pending: 0,
    }
}

fn settings(change: impl FnOnce(&mut Settings)) -> Settings {
    let mut value = default_settings();
    change(&mut value);
    value
}

fn status(vulkan: &Vulkan) -> Status {
    vulkan.status(&fake::LEAVES).0
}

#[test]
fn boot_resets_the_session_under_the_guard() {
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    assert_eq!(fake::take_calls(), ["facts", "start"]);
    assert!(vulkan.booted());
    let value = status(&vulkan);
    assert_eq!(
        (value.struct_size, value.abi_version, value.session_id),
        (size_of::<Status>() as u32, ABI_VERSION, 42)
    );
    assert_eq!(value.fg_result, BACKEND_ERROR_NOT_SUPPORTED);
    assert_eq!(vulkan.settings(), (default_settings(), 1));
    // A booted session refuses another boot.
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert_eq!(fake::take_calls(), Vec::<&str>::new());
}

#[test]
fn boot_refusals() {
    fake::take_calls();
    let vulkan = Vulkan::new();
    let mut missing = setup(c"C:/game/runtime");
    missing.runtime_directory = ptr::null();
    assert_eq!(vulkan.boot(&fake::LEAVES, &missing), 0);
    fake::VK_SUPPORTED.set(false);
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    fake::VK_SUPPORTED.set(true);
    assert!(fake::take_calls().is_empty());
    fake::VK_FACTS.set(VkFacts {
        sdk_shutdown_pending: 1,
        ..VkFacts::default()
    });
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert_eq!(fake::take_calls(), ["facts"]);
    fake::VK_FACTS.set(VkFacts::default());
    fake::VK_START.set(0);
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert!(!vulkan.booted());
    fake::VK_START.set(1);
}

#[test]
fn shutdown_keeps_a_refused_session_and_releases_an_ended_one() {
    fake::VK_RELEASED.set(None);
    let vulkan = booted();
    fake::VK_STOP.set(0);
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 0);
    assert!(vulkan.booted());
    assert_eq!(fake::VK_RELEASED.get(), None);
    fake::VK_STOP.set(1);
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 1);
    assert!(!vulkan.booted());
    assert_eq!(fake::take_calls(), ["stop", "stop", "release"]);
    assert_eq!(fake::VK_RELEASED.get(), Some(42));
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 1);
    assert!(fake::take_calls().is_empty());
    let (value, active) = vulkan.status(&fake::LEAVES);
    assert_eq!(active, 0);
    assert_eq!(
        (value.active_sr, value.sr_result),
        (0, BACKEND_ERROR_NOT_READY)
    );
}

#[test]
fn generation_counts_sr_identity_across_sessions() {
    let vulkan = booted();
    fake::VK_FACTS.set(ready());
    assert_eq!(
        vulkan.configure(&fake::LEAVES, &settings(|s| s.latency_mode = 1)),
        1
    );
    assert_eq!(fake::VK_LATENCY_OPTIONS.get(), Some((1, 0)));
    assert_eq!(vulkan.view().0.generation, 1);
    assert_eq!(
        vulkan.configure(&fake::LEAVES, &settings(|s| s.dlss_preset = 3)),
        1
    );
    assert_eq!(vulkan.view().0.generation, 2);
    vulkan.shutdown(&fake::LEAVES);
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    let (view, active) = vulkan.view();
    assert_eq!(active, 1);
    assert_eq!((view.settings, view.generation), (default_settings(), 2));
    assert_eq!((view.sr_backend, view.fg_backend), (SR_FSR3, FG_FSR3));
}

#[test]
fn an_unbooted_session_refuses_configuration() {
    fake::take_calls();
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.configure(&fake::LEAVES, &default_settings()), 0);
    assert!(fake::take_calls().is_empty());
}

#[test]
fn device_and_provider_refusals_keep_the_request() {
    let vulkan = booted();
    let refusals: [(VkFacts, Settings); 11] = [
        (
            VkFacts {
                xess_device_ready: 0,
                ..ready()
            },
            settings(|s| s.sr_backend = SR_XESS),
        ),
        (
            VkFacts {
                fsr4_sr_compiled: 0,
                ..ready()
            },
            settings(|s| s.sr_backend = SR_FSR4),
        ),
        (
            VkFacts {
                graph_handoff: 0,
                ..ready()
            },
            settings(|s| s.sr_backend = SR_FSR4),
        ),
        (
            VkFacts {
                interop_device_ready: 0,
                ..ready()
            },
            settings(|s| s.sr_backend = SR_FSR4),
        ),
        (
            VkFacts {
                fsr4_fg_compiled: 0,
                ..ready()
            },
            settings(|s| s.fg_backend = FG_FSR4),
        ),
        (
            VkFacts {
                presenter: 0,
                ..ready()
            },
            settings(|s| s.fg_backend = FG_FSR4),
        ),
        (
            ready(),
            settings(|s| {
                s.fg_backend = FG_FSR4;
                s.generated_frames = 2;
            }),
        ),
        (
            VkFacts {
                xess_fg_compiled: 0,
                ..ready()
            },
            settings(|s| s.fg_backend = FG_XESS),
        ),
        (
            VkFacts {
                interop_device_ready: 0,
                ..ready()
            },
            settings(|s| s.fg_backend = FG_XESS),
        ),
        (
            VkFacts {
                presenter: 0,
                ..ready()
            },
            settings(|s| s.fg_mode = 1),
        ),
        (
            ready(),
            settings(|s| {
                s.fg_mode = 1;
                s.generated_frames = 2;
            }),
        ),
    ];
    for (facts, request) in refusals {
        fake::VK_FACTS.set(facts);
        assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0, "{request:?}");
        assert_eq!(vulkan.settings().0, default_settings());
        assert!(!fake::take_calls().contains(&"latency_options"));
    }
    assert_eq!(vulkan.view().0.generation, 1);
}

#[test]
fn interop_fg_needs_the_primary_window_reported_since_the_start() {
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    fake::VK_FACTS.set(ready());
    fake::VK_PROBE.set(1);
    let fsr4 = settings(|s| s.fg_backend = FG_FSR4);
    let xess = settings(|s| s.fg_backend = FG_XESS);
    fake::take_calls();
    // Refused before the probe without a reported window.
    assert_eq!(vulkan.configure(&fake::LEAVES, &fsr4), 0);
    assert_eq!(vulkan.configure(&fake::LEAVES, &xess), 0);
    assert_eq!(fake::take_calls(), ["facts", "facts"]);
    vulkan.report_window(ptr::without_provenance_mut(WINDOW));
    assert_eq!(vulkan.primary_window().addr(), WINDOW);
    assert_eq!(vulkan.configure(&fake::LEAVES, &fsr4), 1);
    assert_eq!(vulkan.configure(&fake::LEAVES, &xess), 1);
    // A refused boot keeps it; the next start clears it, also when the start refuses.
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert_eq!(vulkan.primary_window().addr(), WINDOW);
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 1);
    fake::VK_FACTS.set(VkFacts::default());
    fake::VK_START.set(0);
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 0);
    assert!(vulkan.primary_window().is_null());
    fake::VK_START.set(1);
    fake::VK_PROBE.set(0);
}

#[test]
fn the_fsr4_probe_runs_only_after_the_cheaper_refusals() {
    let vulkan = booted();
    let request = settings(|s| s.fg_backend = FG_FSR4);
    fake::VK_FACTS.set(VkFacts {
        presenter: 0,
        ..ready()
    });
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0);
    assert_eq!(fake::take_calls(), ["facts"]);
    fake::VK_FACTS.set(ready());
    fake::VK_PROBE.set(0);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0);
    assert_eq!(fake::take_calls(), ["facts", "fsr4_fg_probe"]);
    fake::VK_PROBE.set(1);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1);
    assert_eq!(
        fake::take_calls(),
        ["facts", "fsr4_fg_probe", "latency_options"]
    );
    fake::VK_PROBE.set(0);
}

#[test]
fn presenter_capacity_bounds_the_owned_provider() {
    let vulkan = booted();
    fake::VK_FACTS.set(VkFacts {
        presenter_backend: FG_DLSS,
        ..ready()
    });
    let request = settings(|s| {
        s.fg_backend = FG_DLSS;
        s.fg_mode = 1;
        s.generated_frames = 3;
    });
    fake::VK_PRESENTER_MAX.set(None);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0);
    fake::VK_PRESENTER_MAX.set(Some(2));
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0);
    fake::VK_PRESENTER_MAX.set(Some(3));
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1);
    // A different provider is not bounded by the live presenter's capacity.
    fake::VK_PRESENTER_MAX.set(None);
    fake::take_calls();
    let other = settings(|s| {
        s.fg_backend = FG_XESS;
        s.fg_mode = 1;
        s.generated_frames = 3;
    });
    assert_eq!(vulkan.configure(&fake::LEAVES, &other), 1);
    assert_eq!(fake::take_calls(), ["facts", "latency_options"]);
}

#[test]
fn a_refused_latency_option_keeps_the_request() {
    let vulkan = booted();
    fake::VK_FACTS.set(ready());
    fake::VK_LATENCY_RESULT.set(-7);
    assert_eq!(
        vulkan.configure(&fake::LEAVES, &settings(|s| s.quality = 3)),
        0
    );
    assert_eq!(vulkan.view().0.generation, 1);
    fake::VK_LATENCY_RESULT.set(0);
}

#[test]
fn status_carries_the_latency_overlay() {
    let vulkan = booted();
    fake::VK_LATENCY.set(Some(VkLatencyFacts {
        low_latency_available: 1,
        report_available: 1,
        pcl_message: 0xc123,
        mode: 2,
        owner: 3,
        reserved: 0,
        sleep_calls: 5,
        marker_calls: 6,
    }));
    let value = status(&vulkan);
    assert_eq!(
        (
            value.latency_available,
            value.latency_report_available,
            value.pcl_available,
            value.latency_mode,
            value.latency_owner
        ),
        (1, 1, 1, 2, 3)
    );
    assert_eq!((value.sleep_calls, value.marker_calls), (5, 6));
    fake::VK_LATENCY.set(Some(VkLatencyFacts {
        pcl_message: 0xbfff,
        ..VkLatencyFacts::default()
    }));
    assert_eq!(status(&vulkan).pcl_available, 0);
    fake::VK_LATENCY.set(None);
}

fn sr_pass(session: u64, width: u32, height: u32) -> VkFgPass {
    VkFgPass {
        role: RENDER_SR,
        screen: SCREEN_FLIGHT,
        session_id: session,
        view_id: 1,
        render_width: width,
        render_height: height,
        ..VkFgPass::default()
    }
}

#[test]
fn fg_runs_only_in_requested_gameplay_on_the_flight_screen() {
    let vulkan = booted();
    fake::VK_FACTS.set(ready());
    fake::VK_PRESENTER_MAX.set(Some(1));
    let mut pass = sr_pass(42, 1280, 720);
    vulkan.fg_begin(&mut pass);
    assert_eq!((pass.enabled, pass.generated_frames), (0, 1));
    assert_eq!(
        vulkan.configure(&fake::LEAVES, &settings(|s| s.fg_mode = 1)),
        1
    );
    vulkan.fg_begin(&mut pass);
    assert_eq!(pass.enabled, 1);
    pass.screen = 0;
    vulkan.fg_begin(&mut pass);
    assert_eq!(pass.enabled, 0);
    assert_eq!(
        vulkan.configure(
            &fake::LEAVES,
            &settings(|s| {
                s.fg_mode = 1;
                s.gameplay = 0;
            })
        ),
        1
    );
    pass.screen = SCREEN_FLIGHT;
    vulkan.fg_begin(&mut pass);
    assert_eq!(pass.enabled, 0);
    fake::VK_PRESENTER_MAX.set(None);
}

#[test]
fn fixed_guides_retire_on_a_new_plan_and_with_the_presenter() {
    let vulkan = booted();
    vulkan.presenter_created(true, FG_FSR4, 1);
    let value = status(&vulkan);
    assert_eq!(
        (
            value.fg_available,
            value.fg_backend,
            value.max_generated_frames
        ),
        (1, FG_FSR4, 1)
    );
    // The requested provider is FSR3 (Auto): the FSR4 owner must be recreated.
    assert!(!vulkan.presenter_current(FG_FSR4));
    fake::VK_FACTS.set(ready());
    let request = settings(|s| s.fg_backend = FG_FSR4);
    fake::VK_PROBE.set(1);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1);
    assert!(vulkan.presenter_current(FG_FSR4));

    let mut pass = sr_pass(42, 1280, 720);
    pass.options_result = BACKEND_OK;
    pass.backend_valid = 1;
    pass.backend = FG_FSR4;
    assert!(vulkan.fg_admit(&pass));
    vulkan.fg_recorded(&pass, BACKEND_OK);
    assert_eq!(status(&vulkan).fg_result, BACKEND_OK);
    // The same plan keeps the guides.
    assert!(vulkan.fg_admit(&pass));
    // A settled plan of another size retires them through the WSI owner.
    let resized = VkFgPass {
        render_width: 1440,
        render_height: 810,
        ..pass
    };
    assert!(!vulkan.fg_admit(&resized));
    assert_eq!(status(&vulkan).fg_result, BACKEND_ERROR_NOT_READY);
    assert!(!vulkan.presenter_current(FG_FSR4));
    // A new SR plan from settings does the same.
    vulkan.presenter_created(true, FG_FSR4, 1);
    assert!(vulkan.presenter_current(FG_FSR4));
    assert_eq!(
        vulkan.configure(
            &fake::LEAVES,
            &settings(|s| {
                s.fg_backend = FG_FSR4;
                s.quality = 3;
            })
        ),
        1
    );
    assert!(!vulkan.presenter_current(FG_FSR4));
    vulkan.presenter_retired();
    let value = status(&vulkan);
    assert_eq!(
        (
            value.fg_available,
            value.fg_backend,
            value.max_generated_frames
        ),
        (0, 0, 0)
    );
    fake::VK_PROBE.set(0);
}

#[test]
fn a_refused_enable_reports_its_result() {
    let vulkan = booted();
    let mut pass = sr_pass(42, 1280, 720);
    pass.enabled = 1;
    pass.options_result = -5;
    pass.backend_valid = 0;
    assert!(!vulkan.fg_admit(&pass));
    let value = status(&vulkan);
    assert_eq!((value.fg_result, value.generated_frames), (-5, 0));
}

#[test]
fn presents_report_the_presenter_status() {
    let vulkan = booted();
    vulkan.presented(&VkPresentFacts {
        enabled: 1,
        result: BACKEND_OK,
        generated_callbacks: 3,
        presented: 9,
        tagged: 4,
    });
    let value = status(&vulkan);
    assert_eq!(
        (
            value.fg_active,
            value.generated_frames,
            value.total_presented
        ),
        (1, 1, 9)
    );
    assert_eq!((value.generated_callbacks, value.tagged_frames), (3, 4));
}

#[test]
fn sr_frames_count_each_outcome() {
    let vulkan = booted();
    let frame = VkSrFrame {
        identity: 1,
        outcome: SR_EVALUATED,
        result: BACKEND_OK,
        backend: SR_FSR3,
        render_width: 1280,
        render_height: 720,
        output_width: 1920,
        output_height: 1080,
        source_frame_id: 7,
    };
    vulkan.sr_frame(&frame);
    let value = status(&vulkan);
    assert_eq!(
        (value.requested_sr, value.active_sr, value.evaluated_frames),
        (SR_AUTO, SR_FSR3, 1)
    );
    assert_eq!(vulkan.view().0.output_width, 1920);
    // A refused retirement keeps the previous identity.
    vulkan.sr_frame(&VkSrFrame {
        identity: 0,
        outcome: SR_REFUSED,
        result: BACKEND_ERROR_NOT_READY,
        output_width: 2560,
        source_frame_id: 8,
        ..frame
    });
    let value = status(&vulkan);
    assert_eq!(
        (
            value.active_sr,
            value.refused_frames,
            value.output_width,
            value.source_frame_id
        ),
        (0, 1, 1920, 7)
    );
    // A fallback that could not record counts nothing.
    vulkan.sr_frame(&VkSrFrame {
        outcome: 0,
        result: -9,
        ..frame
    });
    let value = status(&vulkan);
    assert_eq!(
        (
            value.sr_result,
            value.evaluated_frames,
            value.refused_frames
        ),
        (-9, 1, 1)
    );
}
