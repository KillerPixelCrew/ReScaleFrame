// SPDX-License-Identifier: MIT
use super::super::SR_FSR3;
use super::*;
use crate::control::default_settings;
use crate::platform::{ABI_VERSION, Settings, Setup, Status, VkFacts, VkFunction, fake};
use std::ffi::c_char;
use std::ptr;

const DEVICE: usize = 0xd0;
const OUTPUT: (u32, u32) = (1920, 1080);

/// Restores every SR leaf answer and drops recorded calls.
fn reset() {
    fake::VK_DRAIN.set(0);
    fake::VK_IDLE.set(1);
    fake::VK_DESTROY.set(0);
    fake::VK_CREATE.set((0, true));
    fake::VK_PLAN.set((1280, 720));
    fake::VK_EVALUATED.set(0);
    fake::VK_NEXT.set(0x7000);
    fake::VK_FALLBACK.set(0);
    fake::VK_THROWS.set(None);
    fake::VK_STOP.set(1);
    fake::VK_FACTS.set(VkFacts {
        graph_handoff: 1,
        fsr4_sr_compiled: 1,
        ..VkFacts::default()
    });
    fake::take_calls();
    fake::take_vk_sr_calls();
}

fn booted() -> Vulkan {
    reset();
    let runtime = c"C:/game/runtime";
    let setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI_VERSION,
        runtime_directory: runtime.as_ptr(),
        engine_version: c"4.7.2".as_ptr(),
        project_id: c"125d2115-7176-46e1-8806-95b790f01717".as_ptr(),
        log: None,
        log_user: ptr::null_mut(),
    };
    let vulkan = Vulkan::new();
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup), 1);
    vulkan.creation().report_ready(true, true);
    fake::take_calls();
    vulkan
}

fn configure(vulkan: &Vulkan, change: impl FnOnce(&mut Settings)) {
    let mut value = default_settings();
    change(&mut value);
    assert_eq!(vulkan.configure(&fake::LEAVES, &value), 1);
    fake::take_calls();
}

unsafe extern "C" fn device_proc(_: *mut c_void, _: *const c_char) -> Option<VkFunction> {
    None
}

fn pass(device: usize, output: (u32, u32)) -> VkSrPass {
    VkSrPass {
        call: ptr::without_provenance_mut(0x10),
        commands: ptr::without_provenance_mut(0x20),
        device: ptr::without_provenance_mut(device),
        device_proc: Some(device_proc),
        output_width: output.0,
        output_height: output.1,
        source_frame_id: 9,
        render_width: 1280,
        render_height: 720,
        handoff: 1,
        split_record: 1,
        continuation: 1,
        fsr4_compiled: 1,
    }
}

fn render(vulkan: &Vulkan, pass: &VkSrPass) -> c_int {
    vulkan.sr_render(&fake::LEAVES, pass)
}

fn status(vulkan: &Vulkan) -> Status {
    vulkan.status(&fake::LEAVES).0
}

fn handles(vulkan: &Vulkan) -> (u64, u64) {
    let (sr, fsr4) = vulkan.sr_contexts();
    (sr.addr() as u64, fsr4.addr() as u64)
}

#[test]
fn a_context_is_created_once_and_reused() {
    let vulkan = booted();
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    let (context, fsr4) = handles(&vulkan);
    assert!(context != 0 && fsr4 == 0);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("create", u64::from(SR_FSR3)),
            ("plan", context),
            ("evaluate", context),
            ("dump", u64::from(SR_FSR3))
        ]
    );
    // Without requested sharpening the Runtime defaults are applied.
    assert_eq!(fake::VK_SHARPENING.get(), Some(fake::sharpening_defaults()));
    let value = status(&vulkan);
    assert_eq!(
        (value.active_sr, value.sr_result, value.evaluated_frames),
        (SR_FSR3, BACKEND_OK, 1)
    );
    assert_eq!(
        (
            value.render_width,
            value.render_height,
            value.output_width,
            value.source_frame_id
        ),
        (1280, 720, 1920, 9)
    );
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("plan", context),
            ("evaluate", context),
            ("dump", u64::from(SR_FSR3))
        ]
    );
}

#[test]
fn a_new_request_output_or_device_retires_the_context_first() {
    let vulkan = booted();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let (first, _) = handles(&vulkan);
    fake::take_vk_sr_calls();
    configure(&vulkan, |s| s.quality = 3);
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    let (second, _) = handles(&vulkan);
    assert_eq!(
        fake::take_vk_sr_calls()[..4],
        [
            ("drain", 0),
            ("idle", DEVICE as u64),
            ("destroy", first),
            ("create", u64::from(SR_FSR3))
        ]
    );
    // The last reported output, not the context's, is compared.
    assert_eq!(render(&vulkan, &pass(DEVICE, (2560, 1440))), 1);
    assert_eq!(fake::take_vk_sr_calls()[2], ("destroy", second));
    // Another device retires on the device the context was made for.
    assert_eq!(render(&vulkan, &pass(0xe0, (2560, 1440))), 1);
    assert_eq!(fake::take_vk_sr_calls()[1], ("idle", DEVICE as u64));
}

#[test]
fn a_refused_retirement_falls_back_before_the_pass_identity() {
    let vulkan = booted();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let kept = handles(&vulkan);
    fake::take_vk_sr_calls();
    fake::VK_IDLE.set(0);
    assert_eq!(render(&vulkan, &pass(DEVICE, (2560, 1440))), 1);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("drain", 0),
            ("idle", DEVICE as u64),
            ("fallback", 0),
            ("log", u64::from(VK_LOG_REJECTED))
        ]
    );
    assert_eq!(handles(&vulkan), kept);
    let value = status(&vulkan);
    assert_eq!(
        (
            value.sr_result,
            value.active_sr,
            value.refused_frames,
            value.output_width
        ),
        (BACKEND_ERROR_NOT_READY, 0, 1, 1920)
    );
}

#[test]
fn a_failed_creation_falls_back_and_retries() {
    let vulkan = booted();
    fake::VK_CREATE.set((-6, false));
    fake::VK_FALLBACK.set(-9);
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 0);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("create", u64::from(SR_FSR3)),
            ("fallback", 0),
            ("log", u64::from(VK_LOG_REJECTED))
        ]
    );
    let value = status(&vulkan);
    assert_eq!((value.sr_result, value.refused_frames), (-6, 1));
    assert_eq!(handles(&vulkan), (0, 0));
    // A context returned with a failed result is kept and retired by the next pass.
    fake::VK_CREATE.set((-6, true));
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let (kept, _) = handles(&vulkan);
    assert_ne!(kept, 0);
    fake::take_vk_sr_calls();
    fake::VK_CREATE.set((0, true));
    fake::VK_FALLBACK.set(0);
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    assert_eq!(
        fake::take_vk_sr_calls()[..3],
        [("drain", 0), ("idle", DEVICE as u64), ("destroy", kept)]
    );
}

#[test]
fn four_refusals_are_logged_per_session() {
    let vulkan = booted();
    fake::VK_CREATE.set((-6, false));
    for _ in 0..6 {
        render(&vulkan, &pass(DEVICE, OUTPUT));
    }
    let logs = fake::take_vk_sr_calls()
        .into_iter()
        .filter(|call| call.0 == "log")
        .count();
    assert_eq!(logs, 4);
    vulkan.shutdown(&fake::LEAVES);
    let runtime = c"C:/game/runtime";
    let setup = Setup {
        struct_size: size_of::<Setup>() as u32,
        abi_version: ABI_VERSION,
        runtime_directory: runtime.as_ptr(),
        engine_version: ptr::null(),
        project_id: ptr::null(),
        log: None,
        log_user: ptr::null_mut(),
    };
    assert_eq!(vulkan.boot(&fake::LEAVES, &setup), 1);
    fake::take_vk_sr_calls();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    assert!(fake::take_vk_sr_calls().contains(&("log", u64::from(VK_LOG_REJECTED))));
}

#[test]
fn an_input_refusal_is_logged_before_the_fallback() {
    let vulkan = booted();
    fake::VK_EVALUATED.set(BACKEND_ERROR_INVALID_ARGUMENT);
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    let calls = fake::take_vk_sr_calls();
    assert_eq!(
        calls[2..],
        [
            ("evaluate", calls[1].1),
            ("dump", u64::from(SR_FSR3)),
            ("log", u64::from(VK_LOG_INPUT_REFUSAL)),
            ("fallback", 0),
            ("log", u64::from(VK_LOG_REJECTED))
        ]
    );
    let value = status(&vulkan);
    assert_eq!(
        (value.sr_result, value.refused_frames, value.render_width),
        (BACKEND_ERROR_INVALID_ARGUMENT, 1, 1280)
    );
}

#[test]
fn passthrough_providers_need_no_context() {
    let vulkan = booted();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let (context, _) = handles(&vulkan);
    fake::take_vk_sr_calls();
    for backend in [SR_NONE, SR_SPATIAL] {
        configure(&vulkan, |s| s.sr_backend = backend);
        assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
        let value = status(&vulkan);
        assert_eq!((value.active_sr, value.sr_result), (backend, BACKEND_OK));
    }
    // The stale context was retired before the first passthrough.
    assert_eq!(
        fake::take_vk_sr_calls(),
        [("drain", 0), ("idle", DEVICE as u64), ("destroy", context)]
    );
    assert_eq!(status(&vulkan).evaluated_frames, 3);
}

#[test]
fn the_dlss_preset_is_applied_once_after_creation() {
    let vulkan = booted();
    configure(&vulkan, |s| {
        s.sr_backend = SR_DLSS;
        s.dlss_preset = 3;
    });
    render(&vulkan, &pass(DEVICE, OUTPUT));
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let calls = fake::take_vk_sr_calls();
    assert_eq!(calls[..2], [("create", u64::from(SR_DLSS)), ("preset", 3)]);
    assert_eq!(calls.iter().filter(|call| call.0 == "preset").count(), 1);
    assert_eq!(status(&vulkan).active_sr, SR_DLSS);
}

#[test]
fn fsr4_needs_its_build_and_the_graph_lease() {
    let vulkan = booted();
    configure(&vulkan, |s| s.sr_backend = SR_FSR4);
    let missing = VkSrPass {
        fsr4_compiled: 0,
        ..pass(DEVICE, OUTPUT)
    };
    render(&vulkan, &missing);
    assert_eq!(status(&vulkan).sr_result, BACKEND_ERROR_NOT_COMPILED);
    for lease in [
        VkSrPass {
            handoff: 0,
            ..pass(DEVICE, OUTPUT)
        },
        VkSrPass {
            split_record: 0,
            ..pass(DEVICE, OUTPUT)
        },
        VkSrPass {
            continuation: 0,
            ..pass(DEVICE, OUTPUT)
        },
    ] {
        render(&vulkan, &lease);
        assert_eq!(status(&vulkan).sr_result, BACKEND_ERROR_NOT_READY);
    }
    let calls = fake::take_vk_sr_calls();
    assert!(
        calls
            .iter()
            .all(|call| call.0 == "fallback" || call.0 == "log")
    );
    assert_eq!(handles(&vulkan), (0, 0));

    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    let (context, fsr4) = handles(&vulkan);
    assert!(context == 0 && fsr4 != 0);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("fsr4_create", u64::from(SR_FSR4)),
            ("fsr4_plan", fsr4),
            ("fsr4_evaluate", fsr4),
            ("dump", u64::from(SR_FSR4))
        ]
    );
    assert_eq!(status(&vulkan).active_sr, SR_FSR4);
    // A live context evaluates only with a handoff and a continuation; split_record is not asked.
    render(
        &vulkan,
        &VkSrPass {
            split_record: 0,
            ..pass(DEVICE, OUTPUT)
        },
    );
    assert_eq!(fake::take_vk_sr_calls()[1], ("fsr4_evaluate", fsr4));
    render(
        &vulkan,
        &VkSrPass {
            handoff: 0,
            continuation: 0,
            ..pass(DEVICE, OUTPUT)
        },
    );
    // The four refusals above spent the session's log budget.
    assert_eq!(
        fake::take_vk_sr_calls(),
        [("fsr4_plan", fsr4), ("fallback", 0)]
    );
    assert_eq!(status(&vulkan).sr_result, BACKEND_ERROR_NOT_READY);
}

#[test]
fn an_fsr4_dispatch_without_a_continuation_is_not_counted() {
    let vulkan = booted();
    configure(&vulkan, |s| s.sr_backend = SR_FSR4);
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let (_, fsr4) = handles(&vulkan);
    fake::take_vk_sr_calls();
    let before = status(&vulkan);
    fake::VK_EVALUATED.set(-9);
    fake::VK_NEXT.set(0);
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 0);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("fsr4_plan", fsr4),
            ("fsr4_evaluate", fsr4),
            ("log", u64::from(VK_LOG_FSR4_DISPATCH)),
            ("dump", u64::from(SR_FSR4))
        ]
    );
    let value = status(&vulkan);
    assert_eq!(value.sr_result, -9);
    assert_eq!(
        (value.evaluated_frames, value.refused_frames),
        (before.evaluated_frames, before.refused_frames)
    );
}

#[test]
fn a_leaf_exception_refuses_the_pass_without_a_report() {
    let vulkan = booted();
    fake::VK_THROWS.set(Some("evaluate"));
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 0);
    assert_eq!(status(&vulkan).evaluated_frames, 0);
    assert_eq!(status(&vulkan).sr_result, BACKEND_OK);
    // A creation that wrote its handle before the exception keeps it for retirement.
    let vulkan = booted();
    fake::VK_THROWS.set(Some("create"));
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 0);
    let (kept, _) = handles(&vulkan);
    assert_ne!(kept, 0);
    fake::VK_THROWS.set(None);
    fake::take_vk_sr_calls();
    assert_eq!(render(&vulkan, &pass(DEVICE, OUTPUT)), 1);
    assert_eq!(fake::take_vk_sr_calls()[2], ("destroy", kept));
}

#[test]
fn shutdown_retires_the_contexts_between_the_presenter_drain_and_the_stop() {
    let vulkan = booted();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    let (context, _) = handles(&vulkan);
    fake::take_vk_sr_calls();
    fake::VK_DESTROY.set(-9);
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 0);
    assert!(vulkan.booted());
    assert!(fake::take_calls().is_empty());
    assert_eq!(handles(&vulkan), (context, 0));
    fake::VK_DESTROY.set(0);
    fake::take_vk_sr_calls();
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 1);
    assert_eq!(
        fake::take_vk_sr_calls(),
        [
            ("drain", 1),
            ("drain", 0),
            ("idle", DEVICE as u64),
            ("destroy", context)
        ]
    );
    assert_eq!(fake::take_calls(), ["stop", "release"]);
    assert_eq!(handles(&vulkan), (0, 0));
    // A refused presenter drain stops before the contexts.
    let vulkan = booted();
    render(&vulkan, &pass(DEVICE, OUTPUT));
    fake::take_vk_sr_calls();
    fake::VK_DRAIN.set(-8);
    assert_eq!(vulkan.shutdown(&fake::LEAVES), 0);
    assert_eq!(fake::take_vk_sr_calls(), [("drain", 1)]);
    fake::VK_DRAIN.set(0);
}
