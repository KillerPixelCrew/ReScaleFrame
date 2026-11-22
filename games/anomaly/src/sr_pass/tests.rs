// SPDX-License-Identifier: MIT
use super::*;
use crate::camera::EngineMatrix;
use crate::sr_stage_abi::{RSF_ANOMALY_SR_PROJECT_ID, RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS};
use rsf_game_sys::{
    RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame, sr_abi::rsf_motion_decode_params,
};
use std::{ffi::c_void, ptr};

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} differs from {b}");
}

fn projection(fov: f32, aspect: f32, near: f32, far: f32) -> EngineMatrix {
    let h = 1.0 / (fov * 0.5).tan();
    let q = far / (far - near);
    [
        h / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        h,
        0.0,
        0.0,
        0.0,
        0.0,
        q,
        1.0,
        0.0,
        0.0,
        -q * near,
        0.0,
    ]
}

fn view_at([px, py, pz]: [f64; 3], yaw: f64) -> EngineMatrix {
    let (s, c) = yaw.sin_cos();
    [
        c as f32,
        0.0,
        s as f32,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        -s as f32,
        0.0,
        c as f32,
        0.0,
        -(px * c - pz * s) as f32,
        -py as f32,
        -(px * s + pz * c) as f32,
        1.0,
    ]
}

fn row_times(v: [f64; 4], matrix: &EngineMatrix) -> [f64; 4] {
    std::array::from_fn(|col| {
        (0..4)
            .map(|row| v[row] * f64::from(matrix[row * 4 + col]))
            .sum()
    })
}

fn source(index: usize) -> *mut c_void {
    ptr::without_provenance_mut(0x1000 + index * 0x100)
}

fn inputs() -> PassInputs {
    let projection = projection(1.2, 16.0 / 9.0, 0.2, 900.0);
    PassInputs {
        session: 0x1234,
        color: source(0),
        color_output: source(1),
        depth: source(2),
        motion: source(3),
        frame: crate::sr_stage_abi::rsf_anomaly_sr_pass {
            native_frame: 7,
            generation: 3,
            render_width: 1280,
            render_height: 720,
            output_width: 1920,
            output_height: 1080,
            current: crate::sr_stage_abi::rsf_anomaly_sr_camera {
                view: view_at([10.0, 2.0, -5.0], 0.3),
                projection,
                valid: true,
            },
            previous: crate::sr_stage_abi::rsf_anomaly_sr_camera {
                view: view_at([10.1, 2.0, -5.0], 0.31),
                projection,
                valid: true,
            },
            jitter: crate::sr_stage_abi::rsf_anomaly_sr_jitter_state {
                frame: 7,
                applied: true,
                current_x: 0.25,
                current_y: -0.125,
                render_width: 1280,
                render_height: 720,
                previous_applied: true,
                previous_x: -0.375,
                previous_y: 0.0625,
                previous_render_width: 1280,
                previous_render_height: 720,
                ..Default::default()
            },
            frame_seconds: 1.0 / 60.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn motion_decode_uses_validity_and_canonical_uv() {
    let p = crate::sr_temporal::motion_decode_params();
    assert_eq!(p.struct_size as usize, size_of_val(&p));
    let v1_size = offset_of!(rsf_motion_decode_params, zero_means_unwritten) + size_of::<u32>();
    assert!(p.struct_size as usize > v1_size);
    assert_eq!(
        [
            p.scale_x,
            p.scale_y,
            p.bias_x,
            p.bias_y,
            p.output_scale_x,
            p.output_scale_y
        ],
        [1.0, 1.0, 0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        (
            p.validity_enabled,
            p.validity_channel,
            p.validity_threshold,
            p.zero_means_unwritten
        ),
        (1, 2, 0.5, 0)
    );
    assert_eq!(p.invalid_value, -1000.0);
    let reference = |stored: [f32; 4]| {
        if stored[p.validity_channel as usize] > p.validity_threshold {
            [
                (stored[0] - p.bias_x) * p.scale_x * p.output_scale_x,
                (stored[1] - p.bias_y) * p.scale_y * p.output_scale_y,
            ]
        } else {
            [p.invalid_value; 2]
        }
    };
    assert_eq!(reference([0.0, 0.0, 1.0, 0.0]), [0.0, 0.0]);
    assert_eq!(reference([0.0125, -0.25, 1.0, 0.0]), [0.0125, -0.25]);
    for invalid in [[0.0; 4], [0.5, 0.5, 0.0, 0.0], [0.1, 0.1, 0.5, 0.0]] {
        assert_eq!(reference(invalid), [-1000.0; 2]);
    }
    assert_eq!(RSF_ANOMALY_SR_VIEW_SPACE_TO_METERS, 1.0);
    assert_eq!(RSF_ANOMALY_SR_PROJECT_ID.len(), 37);
    assert_eq!(RSF_ANOMALY_SR_PROJECT_ID[36], 0);
}

#[test]
fn projection_planes_follow_finite_standard_depth() {
    let p = projection(1.2, 16.0 / 9.0, 0.2, 900.0);
    let planes = camera::projection_planes(&p).unwrap();
    near(f64::from(planes.near), 0.2, 1e-5);
    near(f64::from(planes.far), 900.0, 0.5);
    near(f64::from(planes.vertical_fov), 1.2, 1e-5);
    assert!(camera::projection_planes(&projection(0.9, 1.5, 0.005, 100.0)).is_some());
    let mut ortho = p;
    ortho[11] = 0.0;
    ortho[15] = 1.0;
    let mut inverted = p;
    inverted[14] = 0.1;
    let mut nonfinite = p;
    nonfinite[10] = f32::NAN;
    for bad in [ortho, inverted, nonfinite] {
        assert!(camera::projection_planes(&bad).is_none());
    }
}

#[test]
fn pass_carries_rectangles_identity_and_camera_reprojection() {
    let mut input = inputs();
    let pass = build_pass(&input).unwrap();
    let cam = pass.camera;
    assert_eq!(pass.struct_size as usize, size_of::<rsf_game_render_pass>());
    assert_eq!(cam.struct_size as usize, size_of::<rsf_camera_frame>());
    assert_eq!(cam.abi_version, RSF_GAME_CAMERA_ABI_VERSION);
    assert_eq!(
        (
            pass.role,
            pass.session_id,
            pass.native_frame,
            pass.resource_generation
        ),
        (RSF_GAME_RENDER_SR, 0x1234, 7, 3)
    );
    assert_eq!(
        [
            pass.history_key,
            pass.family_key,
            pass.view_key,
            pass.pass_key
        ],
        [u64::from(RSF_ANOMALY_SR_MAIN_VIEW_KEY); 4]
    );
    assert_eq!(pass.render_rect, [0, 0, 1280, 720]);
    assert_eq!(pass.output_rect, [0, 0, 1920, 1080]);
    assert_eq!(pass.flags, RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG);
    assert_eq!(
        [pass.color_input, pass.color_output, pass.depth, pass.motion],
        [source(0), source(1), source(2), source(3)]
    );
    assert!(pass.exposure.is_null() && pass.ui_input.is_null() && pass.swapchain.is_null());
    assert_eq!(pass.motion_to_uv, [1.0; 2]);
    assert_eq!(
        (
            pass.motion_camera_included,
            pass.camera_valid,
            cam.depth_inverted,
            cam.motion_jittered
        ),
        (0, 1, 0, 0)
    );
    assert_eq!(cam.jitter_pixels, [0.25, -0.125]);
    assert_eq!(cam.previous_jitter_pixels, [-0.375, 0.0625]);
    assert_eq!(
        [
            cam.render_width,
            cam.render_height,
            cam.output_width,
            cam.output_height
        ],
        [1280, 720, 1920, 1080]
    );
    near(f64::from(cam.frame_time_seconds), 1.0 / 60.0, 1e-7);
    near(f64::from(cam.near_plane), 0.2, 1e-5);
    near(f64::from(cam.far_plane), 900.0, 0.5);
    near(f64::from(cam.vertical_fov_radians), 1.2, 1e-5);

    let current = input.frame.current;
    let previous = input.frame.previous;
    assert_eq!(cam.view_to_clip, current.projection);
    assert_eq!(cam.world_to_view, current.view);
    for (actual, expected) in cam.view_to_world[12..15].iter().zip([10.0, 2.0, -5.0]) {
        near(f64::from(*actual), expected, 1e-4);
    }
    near(f64::from(cam.view_to_world[8]), 0.3f64.sin(), 1e-6);
    near(f64::from(cam.view_to_world[10]), 0.3f64.cos(), 1e-6);
    near(f64::from(cam.view_to_world[5]), 1.0, 1e-6);

    let world = [14.0, 3.0, 20.0, 1.0];
    let view = row_times(world, &current.view);
    let clip = row_times(view, &current.projection);
    let previous_clip = row_times(clip, &cam.clip_to_previous_clip);
    let direct_previous = row_times(row_times(world, &previous.view), &previous.projection);
    let rewind = row_times(previous_clip, &pass.previous_clip_to_clip);
    let unprojected = row_times(clip, &cam.clip_to_view);
    for i in 0..3 {
        near(
            previous_clip[i] / previous_clip[3],
            direct_previous[i] / direct_previous[3],
            if i == 2 { 1e-5 } else { 1e-4 },
        );
        near(rewind[i] / rewind[3], clip[i] / clip[3], 1e-4);
        near(unprojected[i] / unprojected[3], view[i], 1e-3);
    }

    input.exposure = source(0);
    input.reset = true;
    let reset = build_pass(&input).unwrap();
    assert_ne!(reset.flags & RSF_GAME_RENDER_RESET, 0);
    assert_eq!(reset.exposure, source(0));
    for (i, value) in reset.camera.clip_to_previous_clip.into_iter().enumerate() {
        near(f64::from(value), if i % 5 == 0 { 1.0 } else { 0.0 }, 1e-4);
    }
    input.reset = false;
    input.frame.previous.valid = false;
    near(
        f64::from(build_pass(&input).unwrap().camera.clip_to_previous_clip[0]),
        1.0,
        1e-4,
    );
    input.frame.jitter.previous_applied = false;
    assert_eq!(
        build_pass(&input).unwrap().camera.previous_jitter_pixels,
        [0.0; 2]
    );
    input.frame.render_width = 1920;
    input.frame.render_height = 1080;
    input.frame.jitter.render_width = 1920;
    input.frame.jitter.render_height = 1080;
    assert_eq!(build_pass(&input).unwrap().render_rect, [0, 0, 1920, 1080]);
}

#[test]
fn refusals_leave_the_native_output_empty() {
    let faults: [(fn(&mut PassInputs), PassError); 14] = [
        (|v| v.frame.render_width = 2000, PassError::Sizes),
        (|v| v.frame.render_height = 0, PassError::Sizes),
        (|v| v.color = ptr::null_mut(), PassError::Resources),
        (|v| v.color_output = ptr::null_mut(), PassError::Resources),
        (|v| v.depth = ptr::null_mut(), PassError::Resources),
        (|v| v.motion = ptr::null_mut(), PassError::Resources),
        (|v| v.frame.current.valid = false, PassError::Camera),
        (|v| v.frame.jitter.applied = false, PassError::Jitter),
        (|v| v.frame.jitter.scope = true, PassError::Jitter),
        (|v| v.frame.jitter.render_width = 1286, PassError::Jitter),
        (
            |v| v.frame.jitter.current_x = f32::INFINITY,
            PassError::Jitter,
        ),
        (
            |v| v.frame.current.projection[11] = 0.0,
            PassError::Projection,
        ),
        (|v| v.frame.current.view = [0.0; 16], PassError::Camera),
        (|v| v.frame.previous.view = [0.0; 16], PassError::Camera),
    ];
    for (fault, expected) in faults {
        let mut input = inputs();
        let mut output = MaybeUninit::new(build_pass(&input).unwrap());
        fault(&mut input);
        assert_eq!(build_pass(&input).err(), Some(expected));
        assert_eq!(
            rsf_anomaly_sr_build_pass(&input, &mut output),
            expected as u8
        );
        let output = unsafe { output.assume_init() };
        assert_eq!(
            (output.struct_size, output.role, output.camera_valid),
            (0, 0, 0)
        );
        assert!(output.color_input.is_null());
    }
    let mut input = inputs();
    input.frame.previous.view = [0.0; 16];
    input.reset = true;
    let reset = build_pass(&input).unwrap();
    assert_ne!(reset.flags & RSF_GAME_RENDER_RESET, 0);
    assert_eq!(
        reset.camera.previous_jitter_pixels,
        [input.frame.jitter.previous_x, input.frame.jitter.previous_y]
    );
}

#[test]
fn reset_precedence_and_camera_cut_thresholds() {
    let mut facts = ResetFacts {
        first: true,
        device: true,
        settings: true,
        sequence: true,
        cut: true,
    };
    assert_eq!(reset_reason(&facts), ResetReason::First);
    facts.first = false;
    assert_eq!(reset_reason(&facts), ResetReason::Device);
    facts.device = false;
    assert_eq!(reset_reason(&facts), ResetReason::Settings);
    facts.settings = false;
    assert_eq!(reset_reason(&facts), ResetReason::Sequence);
    facts.sequence = false;
    assert_eq!(reset_reason(&facts), ResetReason::Cut);
    facts.cut = false;
    assert_eq!(reset_reason(&facts), ResetReason::None);

    let current = camera::pose(&view_at([0.0; 3], 0.0));
    assert!(current.valid);
    near(current.forward[2], 1.0, 1e-6);
    for (position, yaw, cut) in [
        ([0.1, 0.0, 0.05], 0.02, false),
        ([3.9, 0.0, 0.0], 0.0, false),
        ([4.1, 0.0, 0.0], 0.0, true),
        ([0.0; 3], 0.7, false),
        ([0.0; 3], 0.9, true),
    ] {
        assert_eq!(
            camera::camera_cut(current, camera::pose(&view_at(position, yaw)), 4.0, 45.0),
            cut
        );
    }
    let invalid = camera::Pose::default();
    assert!(camera::camera_cut(invalid, current, 4.0, 45.0));
    assert!(camera::camera_cut(current, invalid, 4.0, 45.0));
    assert!(!camera::camera_cut(
        current,
        camera::pose(&view_at([8.0, 0.0, 0.0], 0.0)),
        10.0,
        45.0
    ));
}
