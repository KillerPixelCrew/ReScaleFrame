// SPDX-License-Identifier: MIT
// Rust port of the synthetic RAGE camera checks in tests/rdr2_camera.cpp.
use super::*;

const JX: f32 = 0.25;
const JY: f32 = -0.388_888_9;

fn put(view: &mut [u8], offset: usize, value: f32) {
    view[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_matrix(view: &mut [u8], offset: usize, matrix: &[f32; 16]) {
    for (index, value) in matrix.iter().enumerate() {
        put(view, offset + index * 4, *value);
    }
}

fn identity_f32() -> [f32; 16] {
    let mut matrix = [0.0; 16];
    for index in 0..4 {
        matrix[5 * index] = 1.0;
    }
    matrix
}

struct Fixture {
    view: Vec<u8>,
    projection: [f32; 16],
    input: CameraInputs,
}

fn fixture() -> Fixture {
    let mut view = vec![0u8; 0x800];
    let (mut current, mut world) = (identity_f32(), identity_f32());
    current[12] = -10.0;
    current[13] = 20.0;
    current[14] = -30.0;
    world[12] = 10.0;
    world[13] = -20.0;
    world[14] = 30.0;
    let mut previous = current;
    previous[12] = -9.0;
    let (near, far, fov) = (0.15f32, 10000.0f32, 0.66f32);
    let focal = 1.0 / (fov / 2.0).tan();
    let mut projection = [0.0f32; 16];
    projection[0] = focal / 1.6;
    projection[5] = focal;
    projection[8] = -2.0 * JX / 1706.0;
    projection[9] = 2.0 * JY / 1066.0;
    projection[10] = near / (far - near);
    projection[11] = -1.0;
    projection[14] = near * far / (far - near);
    put_matrix(&mut view, 0x40, &current);
    put_matrix(&mut view, 0x80, &projection);
    put_matrix(&mut view, 0xc0, &world);
    put_matrix(&mut view, 0x140, &previous);
    put_matrix(&mut view, 0x180, &projection);
    put(&mut view, 0x630, near);
    put(&mut view, 0x634, far);
    put(&mut view, 0x640, fov * 180.0 / std::f32::consts::PI);
    put(&mut view, 0x664, projection[8]);
    put(&mut view, 0x668, projection[9]);
    put(&mut view, 0x67c, 1706.0);
    put(&mut view, 0x680, 1066.0);
    view[0x671] = 1;
    let input = CameraInputs {
        render_width: 1706,
        render_height: 1066,
        output_width: 2560,
        output_height: 1600,
        jitter: [JX, JY],
        previous_jitter: [-0.25, 0.166_666_67],
        frame_time_seconds: 1.0 / 60.0,
        reset: false,
        motion_jittered: false,
    };
    Fixture {
        view,
        projection,
        input,
    }
}

#[test]
fn decodes_a_valid_rage_camera() {
    let fixture = fixture();
    let mut output = CameraResult::default();
    assert!(decode(&fixture.view, &fixture.input, &mut output));
    assert!(
        output.projection_jitter_matches,
        "NGX jitter agrees with the producer"
    );
    let camera = &output.camera;
    assert_eq!(camera.struct_size as usize, size_of::<rsf_camera_frame>());
    assert_eq!(camera.view_to_clip[8], 0.0);
    assert_eq!(camera.view_to_clip[9], 0.0);
    for r in 0..4 {
        for c in 0..4 {
            let sum: f64 = (0..4)
                .map(|k| {
                    f64::from(camera.view_to_clip[r * 4 + k])
                        * f64::from(camera.clip_to_view[k * 4 + c])
                })
                .sum();
            let expected = if r == c { 1.0 } else { 0.0 };
            assert!(
                (sum - expected).abs() < 1e-6,
                "projection times returned inverse"
            );
        }
    }
    // World point (10,-20,20): current view (0,0,-10), previous view (1,0,-10).
    let p = &fixture.projection;
    let clip = [
        0.0,
        0.0,
        (-10.0 * f64::from(p[10]) + f64::from(p[14])) / 10.0,
        1.0,
    ];
    let mut transformed = [0.0f64; 4];
    for (c, value) in transformed.iter_mut().enumerate() {
        *value = (0..4)
            .map(|r| clip[r] * f64::from(camera.clip_to_previous_clip[r * 4 + c]))
            .sum();
    }
    assert!(
        (transformed[0] / transformed[3] - f64::from(p[0]) / 10.0).abs() < 1e-6,
        "known static point reprojects to the previous camera"
    );
    assert!((camera.vertical_fov_radians - 0.66).abs() < 1e-5);
    assert_eq!(camera.depth_inverted, 1);
}

#[test]
fn reset_clears_reprojection_and_seeds_jitter_history() {
    let mut fixture = fixture();
    fixture.input.reset = true;
    let mut output = CameraResult::default();
    assert!(decode(&fixture.view, &fixture.input, &mut output));
    for (index, value) in output.camera.clip_to_previous_clip.iter().enumerate() {
        assert_eq!(*value, if index % 5 == 0 { 1.0 } else { 0.0 });
    }
    assert_eq!(output.camera.previous_jitter_pixels[0], JX);
}

#[test]
fn frozen_projection_jitter_stays_explicit() {
    let mut fixture = fixture();
    fixture.input.jitter[0] = -0.25;
    let mut output = CameraResult::default();
    assert!(decode(&fixture.view, &fixture.input, &mut output));
    assert!(!output.projection_jitter_matches);
}

#[test]
fn refusals_leave_a_zeroed_result() {
    let mut fixture = fixture();
    let mut output = CameraResult::default();
    fixture.input.jitter[0] = f32::NAN;
    assert!(
        !decode(&fixture.view, &fixture.input, &mut output),
        "non-finite vendor jitter"
    );
    fixture.input.jitter[0] = JX;
    assert!(
        !decode(&fixture.view[..16], &fixture.input, &mut output),
        "short copied view"
    );
    let mut inactive = fixture.view.clone();
    inactive[0x671] = 0;
    assert!(
        !decode(&inactive, &fixture.input, &mut output),
        "inactive view"
    );
    // Row 0 of the projection becomes zero.
    put(&mut fixture.view, 0x80, 0.0);
    assert!(
        !decode(&fixture.view, &fixture.input, &mut output),
        "singular projection"
    );
    assert_eq!(output.camera.struct_size, 0);
    assert!(!output.projection_jitter_matches);
}

#[test]
fn native_abi_matches_the_safe_decode() {
    let fixture = fixture();
    let mut output = CameraResult::default();
    let accepted = unsafe {
        rsf_rdr2_decode_render_camera_abi(
            fixture.view.as_ptr().cast(),
            fixture.view.len(),
            &fixture.input,
            &mut output,
        )
    };
    assert!(accepted && output.projection_jitter_matches);
    assert!(!unsafe {
        rsf_rdr2_decode_render_camera_abi(std::ptr::null(), 0, &fixture.input, &mut output)
    });
    assert_eq!(output.camera.struct_size, 0);
}
