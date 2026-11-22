// SPDX-License-Identifier: MIT
use super::*;

fn put(view: &mut [u8], offset: usize, value: f32) {
    view[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

struct Fixture {
    view: Vec<u8>,
    projection: [f32; 16],
    input: CameraInputs,
    jitter_matches: bool,
    fov: f32,
}

fn fixture() -> Fixture {
    let bytes = include_bytes!("../../../../tests/fixtures/rdr2-camera-v1.bin");
    assert_eq!(&bytes[..8], b"RSFCAMF1");
    let view = bytes[8..8 + 0x800].to_vec();
    let input_offset = 8 + view.len();
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let integer = |offset| word(input_offset + offset);
    let real = |offset| f32::from_bits(integer(offset));
    Fixture {
        projection: std::array::from_fn(|index| f32::from_bits(word(8 + 0x80 + index * 4))),
        view,
        input: CameraInputs {
            render_width: integer(0),
            render_height: integer(4),
            output_width: integer(8),
            output_height: integer(12),
            jitter: [real(16), real(20)],
            previous_jitter: [real(24), real(28)],
            frame_time_seconds: real(32),
            reset: false,
            motion_jittered: false,
        },
        jitter_matches: integer(40) != 0,
        fov: real(44),
    }
}

#[test]
fn decodes_a_valid_rage_camera() {
    let fixture = fixture();
    let mut output = CameraResult::default();
    assert!(decode(&fixture.view, &fixture.input, &mut output));
    assert_eq!(output.projection_jitter_matches, fixture.jitter_matches);
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
    assert!((camera.vertical_fov_radians - fixture.fov).abs() < 1e-5);
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
    assert_eq!(
        output.camera.previous_jitter_pixels[0],
        fixture.input.jitter[0]
    );
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
    let original_jitter = fixture.input.jitter[0];
    fixture.input.jitter[0] = f32::NAN;
    assert!(
        !decode(&fixture.view, &fixture.input, &mut output),
        "non-finite vendor jitter"
    );
    fixture.input.jitter[0] = original_jitter;
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
