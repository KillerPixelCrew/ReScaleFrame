// SPDX-License-Identifier: MIT
use super::*;
use crate::platform::{VkAdapter, fake};
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
    vulkan.creation().report_ready(true, true);
    fake::take_calls();
    vulkan
}

/// Every provider and presenter fact present. Device readiness is the creation owner's.
fn ready() -> VkFacts {
    VkFacts {
        reserved_creation: [0; 2],
        graph_handoff: 1,
        fsr4_sr_compiled: 1,
        fsr4_fg_compiled: 1,
        xess_fg_compiled: 1,
        presenter: 1,
        streamline_ready: 0,
        presenter_backend: FG_FSR3,
        sdk_shutdown_pending: 0,
    }
}

fn adapter(vendor: u32, software: u32) -> Option<VkAdapter> {
    Some(VkAdapter {
        vendor,
        device: 0x1234,
        software,
        reserved: 0,
    })
}

fn bits(ids: &[u32]) -> u32 {
    ids.iter().map(|&id| 1u32 << id).sum()
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
    assert_eq!(vulkan.view(&fake::LEAVES).0.generation, 1);
    assert_eq!(
        vulkan.configure(&fake::LEAVES, &settings(|s| s.dlss_preset = 3)),
        1
    );
    assert_eq!(vulkan.view(&fake::LEAVES).0.generation, 2);
    vulkan.shutdown(&fake::LEAVES);
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    let (view, active) = vulkan.view(&fake::LEAVES);
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
    let refusals: [(VkFacts, Settings); 8] = [
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
    // XeSS SR needs the XeSS device, FSR4 SR and XeSS FG the Win32 interop device, as the
    // creation owner last created them.
    let unready = [
        ((false, true), settings(|s| s.sr_backend = SR_XESS)),
        ((true, false), settings(|s| s.sr_backend = SR_FSR4)),
        ((true, false), settings(|s| s.fg_backend = FG_XESS)),
    ];
    fake::VK_FACTS.set(ready());
    for ((xess, interop), request) in unready {
        vulkan.creation().report_ready(xess, interop);
        assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0, "{request:?}");
        assert_eq!(vulkan.settings().0, default_settings());
        assert!(!fake::take_calls().contains(&"latency_options"));
        vulkan.creation().report_ready(true, true);
        assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1, "{request:?}");
        assert_eq!(vulkan.configure(&fake::LEAVES, &default_settings()), 1);
        fake::take_calls();
    }
    assert_eq!(vulkan.view(&fake::LEAVES).0.generation, 5);
}

#[test]
fn interop_fg_needs_the_primary_window_reported_since_the_start() {
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    vulkan.creation().report_ready(true, true);
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
    assert_eq!(vulkan.view(&fake::LEAVES).0.generation, 1);
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
    assert!(!vulkan.presenter_current(&fake::LEAVES, FG_FSR4));
    fake::VK_FACTS.set(ready());
    let request = settings(|s| s.fg_backend = FG_FSR4);
    fake::VK_PROBE.set(1);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1);
    assert!(vulkan.presenter_current(&fake::LEAVES, FG_FSR4));

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
    assert!(!vulkan.presenter_current(&fake::LEAVES, FG_FSR4));
    // A new SR plan from settings does the same.
    vulkan.presenter_created(true, FG_FSR4, 1);
    assert!(vulkan.presenter_current(&fake::LEAVES, FG_FSR4));
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
    assert!(!vulkan.presenter_current(&fake::LEAVES, FG_FSR4));
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
    assert_eq!(vulkan.view(&fake::LEAVES).0.output_width, 1920);
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

#[test]
fn auto_resolves_through_the_runtime_policy_for_the_device_adapter() {
    let vulkan = booted();
    let served = VkFacts {
        streamline_ready: 1,
        ..ready()
    };
    fake::VK_FACTS.set(served);
    fake::take_auto();
    // Every provider servable: SR DLSS, FSR3, FSR4 and XeSS; FG DLSS, FSR3 and XeSS.
    let sr_all = bits(&[SR_DLSS, SR_FSR3, SR_FSR4, SR_XESS]);
    let fg_all = bits(&[FG_DLSS, FG_FSR3, FG_XESS]);
    let cases = [
        (adapter(0x10de, 0), (SR_DLSS, FG_DLSS)),
        (adapter(0x8086, 0), (SR_XESS, FG_XESS)),
        (adapter(0x1002, 0), (SR_FSR3, FG_FSR3)),
        (adapter(0x10de, 1), (0, FG_FSR3)),
        // No adapter facts: the Runtime's unknown adapter, which keeps the former FSR3.
        (None, (SR_FSR3, FG_FSR3)),
    ];
    for (facts, expected) in cases {
        vulkan.creation().report_adapter(facts);
        let view = vulkan.view(&fake::LEAVES).0;
        assert_eq!((view.sr_backend, view.fg_backend), expected, "{facts:?}");
        let identity = facts.map_or((0, 0), |value| (value.vendor, value.software));
        assert_eq!(
            fake::take_auto(),
            (
                Some((identity.0, identity.1, sr_all)),
                Some((identity.0, identity.1, fg_all))
            )
        );
    }
    // Explicit requests pass unchanged and ask neither the policy nor the facts.
    vulkan.creation().report_adapter(adapter(0x10de, 0));
    assert_eq!(
        vulkan.configure(
            &fake::LEAVES,
            &settings(|s| {
                s.sr_backend = SR_FSR3;
                s.fg_backend = FG_XESS;
            })
        ),
        1
    );
    fake::take_calls();
    fake::take_auto();
    let view = vulkan.view(&fake::LEAVES).0;
    assert_eq!((view.sr_backend, view.fg_backend), (SR_FSR3, FG_XESS));
    assert_eq!(fake::take_auto(), (None, None));
    assert!(fake::take_calls().is_empty());
}

#[test]
fn auto_chooses_only_providers_the_session_can_serve() {
    let vulkan = booted();
    // NVIDIA without the Streamline session: DLSS is not served, FSR3 remains.
    vulkan.creation().report_adapter(adapter(0x10de, 0));
    fake::VK_FACTS.set(ready());
    let view = vulkan.view(&fake::LEAVES).0;
    assert_eq!((view.sr_backend, view.fg_backend), (SR_FSR3, FG_FSR3));
    // Intel without the XeSS device, the interop device or the primary window.
    vulkan.creation().report_adapter(adapter(0x8086, 0));
    vulkan.creation().report_ready(false, false);
    fake::take_auto();
    let view = vulkan.view(&fake::LEAVES).0;
    assert_eq!((view.sr_backend, view.fg_backend), (SR_FSR3, FG_FSR3));
    assert_eq!(
        fake::take_auto(),
        (
            Some((0x8086, 0, bits(&[SR_FSR3]))),
            Some((0x8086, 0, bits(&[FG_FSR3])))
        )
    );
    // The XeSS device serves XeSS SR; XeSS FG also needs interop and the primary window.
    vulkan.creation().report_ready(true, true);
    let fresh = Vulkan::new();
    assert_eq!(fresh.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    fresh.creation().report_adapter(adapter(0x8086, 0));
    fresh.creation().report_ready(true, true);
    let view = fresh.view(&fake::LEAVES).0;
    assert_eq!((view.sr_backend, view.fg_backend), (SR_XESS, FG_FSR3));
    fresh.report_window(ptr::without_provenance_mut(WINDOW));
    assert_eq!(fresh.view(&fake::LEAVES).0.fg_backend, FG_XESS);
    // FSR4 SR is served only with its build, the graph lease capability and interop; FSR4 FG is
    // never offered to Auto, so its probe is not asked.
    fake::VK_FACTS.set(VkFacts {
        graph_handoff: 0,
        ..ready()
    });
    fake::take_auto();
    fresh.view(&fake::LEAVES);
    let (sr, fg) = fake::take_auto();
    assert_eq!(sr.map(|asked| asked.2 & bits(&[SR_FSR4])), Some(0));
    assert_eq!(fg.map(|asked| asked.2 & bits(&[FG_FSR4])), Some(0));
    assert!(!fake::take_calls().contains(&"fsr4_fg_probe"));
}

#[test]
fn auto_admission_and_presenter_follow_the_resolved_provider() {
    let vulkan = booted();
    vulkan.creation().report_adapter(adapter(0x10de, 0));
    fake::VK_FACTS.set(VkFacts {
        streamline_ready: 1,
        presenter_backend: FG_FSR3,
        ..ready()
    });
    // Auto FG resolves to DLSS: FSR3's single-frame rule does not apply, the live FSR3
    // presenter's capacity is not asked, and the FSR3 presenter is no longer current.
    let request = settings(|s| {
        s.fg_mode = 1;
        s.generated_frames = 3;
    });
    fake::VK_PRESENTER_MAX.set(None);
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 1);
    assert!(!vulkan.presenter_current(&fake::LEAVES, FG_FSR3));
    assert!(vulkan.presenter_current(&fake::LEAVES, FG_DLSS));
    // Without the Streamline session Auto keeps FSR3, whose single-frame rule refuses.
    fake::VK_FACTS.set(VkFacts {
        presenter_backend: FG_FSR3,
        ..ready()
    });
    assert_eq!(vulkan.configure(&fake::LEAVES, &request), 0);
    assert!(vulkan.presenter_current(&fake::LEAVES, FG_FSR3));
    // Auto FG on Intel before the primary window is reported resolves to FSR3 instead of
    // refusing the request for XeSS's missing window.
    let intel = Vulkan::new();
    assert_eq!(intel.boot(&fake::LEAVES, &setup(c"C:/game/runtime")), 1);
    intel.creation().report_adapter(adapter(0x8086, 0));
    intel.creation().report_ready(true, true);
    fake::VK_FACTS.set(ready());
    fake::VK_PRESENTER_MAX.set(Some(1));
    assert_eq!(
        intel.configure(&fake::LEAVES, &settings(|s| s.fg_mode = 1)),
        1
    );
    assert!(intel.presenter_current(&fake::LEAVES, FG_FSR3));
    // Once reported, XeSS is served: the FSR3 owner is recreated as a fixed-guide XeSS owner.
    intel.report_window(ptr::without_provenance_mut(WINDOW));
    assert!(!intel.presenter_current(&fake::LEAVES, FG_FSR3));
    intel.presenter_created(true, FG_XESS, 1);
    assert!(intel.presenter_current(&fake::LEAVES, FG_XESS));
    fake::VK_PRESENTER_MAX.set(None);
}
