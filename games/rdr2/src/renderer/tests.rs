// SPDX-License-Identifier: MIT
use super::*;

fn host() -> Host {
    // No callbacks; every service field is an empty option or null.
    let mut services: rsf_game_host_services = unsafe { std::mem::zeroed() };
    services.session_id = 9;
    Host(services)
}

fn descriptor() -> Evaluation {
    // The native record's reserved fields stay private to the layout module.
    let mut descriptor = Evaluation::default();
    descriptor.color = 0x10 as *mut c_void;
    descriptor.output = 0x20 as *mut c_void;
    descriptor.depth = 0x30 as *mut c_void;
    descriptor.motion = 0x40 as *mut c_void;
    descriptor.jitter = [0.25, -0.5];
    descriptor.width = 1706;
    descriptor.height = 1066;
    descriptor.motion_scale = [1706.0, 1066.0];
    descriptor
}

fn request() -> Request {
    Request {
        list: 0x50 as *mut c_void,
        feature: 0x60,
        flags: NGX_DEPTH_INVERTED,
        engine_states: None,
    }
}

#[test]
fn native_history_recovers_only_after_successful_fallback() {
    let mut history = History::default();
    assert_eq!(
        (history.frame, history.native_gap, history.native_result),
        (0, false, u32::MAX)
    );
    history.frame = 40;
    history.replaced();
    assert!(history.native_gap);
    assert!(history.fell_back(NGX_FAIL, false));
    assert_eq!(history.frame, 0);
    assert!(history.native_gap);
    assert!(history.fell_back(NGX_SUCCESS, true));
    assert!(!history.native_gap);
    assert!(!history.fell_back(NGX_SUCCESS, true));
}

#[test]
fn fsr_fallback_passes_the_full_copy_and_leaves_the_caller_untouched() {
    let mut original = [0x5a; fsr::FULL_DESCRIPTION_BYTES];
    original[fsr::RESET] = 0;
    let mut copy = FsrDescription(original);
    let result = copy.fallback(original.as_ptr().cast(), true, |pointer| {
        let bytes = unsafe { &*pointer.cast::<[u8; fsr::FULL_DESCRIPTION_BYTES]>() };
        assert_ne!(bytes.as_ptr(), original.as_ptr());
        assert_eq!(bytes[fsr::RESET], 1);
        assert_eq!(&bytes[..fsr::RESET], &original[..fsr::RESET]);
        assert_eq!(&bytes[fsr::RESET + 1..], &original[fsr::RESET + 1..]);
        fsr::FFX_OK
    });
    assert_eq!(result, fsr::FFX_OK);
    assert_eq!(original[fsr::RESET], 0);
}

#[test]
fn fsr_flags_require_supported_depth_and_motion_domains() {
    assert_eq!(fsr_camera_flags(0), None);
    assert_eq!(
        fsr_camera_flags(fsr::INVERTED_DEPTH | fsr::DISPLAY_MOTION),
        None
    );
    assert_eq!(
        fsr_camera_flags(fsr::INVERTED_DEPTH | fsr::INFINITE_DEPTH),
        None
    );
    assert_eq!(
        fsr_camera_flags(fsr::INVERTED_DEPTH),
        Some(NGX_DEPTH_INVERTED)
    );
    assert_eq!(
        fsr_camera_flags(fsr::INVERTED_DEPTH | fsr::JITTERED_MOTION),
        Some(NGX_DEPTH_INVERTED | NGX_MV_JITTERED)
    );
}

#[test]
fn unsupported_descriptors_refuse() {
    let request = request();
    assert!(!unsupported(&request, &descriptor()));
    let mut d = descriptor();
    d.invert_y = 1;
    assert!(unsupported(&request, &d));
    let mut d = descriptor();
    d.pre_exposure = 0.5;
    assert!(unsupported(&request, &d));
    d.pre_exposure = 1.0;
    assert!(!unsupported(&request, &d));
    let mut d = descriptor();
    d.motion = ptr::null_mut();
    assert!(unsupported(&request, &d));
    let mut d = descriptor();
    d.height = 0;
    assert!(unsupported(&request, &d));
    let mut detached = request;
    detached.feature = 0;
    assert!(unsupported(&detached, &descriptor()));
}

#[test]
fn sr_pass_carries_identity_motion_scale_and_fg_refusal() {
    let mut output = ResourceFacts::default();
    output.width = 2560;
    output.height = 1600;
    output.format = d3d::R16G16B16A16_FLOAT;
    output.samples = 1;
    let decoded = CameraResult::default();
    let d = descriptor();
    let pass = sr_pass(
        &host(),
        &request(),
        &d,
        &output,
        &decoded,
        (0x70, 41, true, 0),
    );
    assert_eq!(pass.struct_size as usize, size_of::<rsf_game_render_pass>());
    assert_eq!(pass.role, RSF_GAME_RENDER_SR);
    assert_eq!(pass.session_id, 9);
    assert_eq!(
        (pass.family_key, pass.view_key, pass.history_key),
        (0x60, 0x60, 0x60)
    );
    assert_eq!(
        (pass.pass_key, pass.native_frame, pass.scope_id),
        (41, 41, 0x70)
    );
    assert_eq!(
        pass.flags,
        RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_RESET | RSF_GAME_RENDER_NO_FG
    );
    assert_eq!(pass.render_rect, [0, 0, 1706, 1066]);
    assert_eq!(pass.output_rect, [0, 0, 2560, 1600]);
    assert_eq!(pass.motion_to_uv, [-1.0, -1.0]);
    assert_eq!((pass.screen, pass.source_frame_id), (RSF_SCREEN_UNKNOWN, 0));
    assert_eq!((pass.camera_valid, pass.motion_camera_included), (1, 1));
    let flight = sr_pass(
        &host(),
        &request(),
        &d,
        &output,
        &decoded,
        (0x70, 41, false, 41),
    );
    assert_eq!(flight.flags, RSF_GAME_RENDER_PRIMARY);
    assert_eq!(
        (flight.screen, flight.source_frame_id),
        (RSF_SCREEN_FLIGHT, 41)
    );
}

#[test]
fn transitions_hand_the_host_readable_inputs_and_a_copy_target() {
    assert_eq!(HOST_STATES[3], d3d::COPY_DEST);
    assert_eq!(DLSS_STATES[1], d3d::DEPTH_READ);
    assert!(
        HOST_STATES
            .iter()
            .enumerate()
            .all(|(index, &state)| index == 3 || state == d3d::NON_PIXEL_SHADER_RESOURCE)
    );
}
