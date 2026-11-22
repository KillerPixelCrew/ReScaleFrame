// SPDX-License-Identifier: MIT
use super::*;

fn apply(point: &[f32; 4], m: &Flat) -> [f32; 4] {
    std::array::from_fn(|j| (0..4).map(|i| point[i] * m[i * 4 + j]).sum())
}

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() < tolerance
}

fn fixture_camera() -> NativeCamera {
    let mut projection = [0.0; 16];
    projection[0] = 2.0;
    projection[5] = 3.0;
    projection[8] = 0.2;
    projection[9] = -0.1;
    projection[10] = 1.01;
    projection[11] = 1.0;
    projection[14] = -1.01;
    let mut native = NativeCamera {
        view: xm::flatten(&xm::IDENTITY),
        projection,
        view_projection_unjittered: projection,
        previous_view_projection_unjittered: projection,
        position_adjust: [0.0; 3],
        previous_position_adjust: [0.0; 3],
        jitter_ndc: [0.0; 2],
    };
    native.projection[8] += 0.0007;
    native.projection[9] -= 0.0011;
    native
}

fn run(
    native: &NativeCamera,
    current: Option<&CameraRaster>,
    previous: Option<&CameraRaster>,
) -> (bool, rsf_camera_frame, bool) {
    let mut camera: rsf_camera_frame = unsafe { std::mem::zeroed() };
    let mut backward = [0.0; 16];
    let mut history = false;
    let admitted = frame(
        native,
        99,
        80,
        current,
        previous,
        FrameOutputs {
            camera: &mut camera,
            previous_clip_to_clip: &mut backward,
            history_valid: &mut history,
        },
    );
    (admitted, camera, history)
}

#[test]
fn inverse_follows_directxmath_on_exact_cases() {
    let diagonal = [
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 4.0, 0.0, 0.0],
        [0.0, 0.0, 8.0, 0.0],
        [0.0, 0.0, 0.0, 16.0],
    ];
    let (inverse, determinant) = xm::inverse(&diagonal);
    assert_eq!(determinant, 1024.0);
    assert_eq!(
        xm::flatten(&inverse),
        [
            0.5, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 0.0625
        ]
    );
    let (inverse, _) = xm::inverse(&xm::translation(3.0, -5.0, 7.0));
    assert_eq!(inverse, xm::translation(-3.0, 5.0, -7.0));
}

#[test]
fn determinant_threshold_is_the_directxmath_one() {
    // det 1e-12 is invertible to the GameSDK f64 pivot floor but refused here.
    let small = xm::load(&[
        1e-3, 0.0, 0.0, 0.0, 0.0, 1e-3, 0.0, 0.0, 0.0, 0.0, 1e-3, 0.0, 0.0, 0.0, 0.0, 1e-3,
    ]);
    assert!(invert(&small).is_none());
    assert!(invert(&xm::IDENTITY).is_some());
    assert!(invert(&[[0.0; 4]; 4]).is_none());
}

#[test]
fn fractional_raster_projects_pixel_centers_to_the_crop_grid() {
    let native = fixture_camera();
    let raster = CameraRaster {
        xywh: [0.75, 1.25, 99.2, 79.6],
        crop: [1, 1, 100, 81],
    };
    let (admitted, camera, history) = run(&native, Some(&raster), Some(&raster));
    assert!(admitted && history);
    let (px, py, z) = (42.5_f32, 25.5_f32, 5.0_f32);
    let nx = 2.0 * (px - 0.75) / 99.2 - 1.0;
    let ny = 1.0 - 2.0 * (py - 1.25) / 79.6;
    let point = [(nx - 0.2) * z / 2.0, (ny + 0.1) * z / 3.0, z, 1.0];
    let clip = apply(&point, &camera.view_to_clip);
    assert!(close(
        clip[0] / clip[3],
        2.0 * (px - 1.0) / 99.0 - 1.0,
        1e-4
    ));
    assert!(close(
        clip[1] / clip[3],
        1.0 - 2.0 * (py - 1.0) / 80.0,
        1e-4
    ));
    assert!(close(camera.jitter_pixels[0], 0.0007 * 99.2 / 2.0, 1e-4));
    assert!(close(camera.jitter_pixels[1], 0.0011 * 79.6 / 2.0, 1e-4));
    assert!(close(camera.near_plane, 1.0, 1e-4) && close(camera.far_plane, 101.0, 0.002));

    let restored = apply(&clip, &camera.clip_to_view);
    for (&value, expected) in restored.iter().zip(point).take(3) {
        assert!(close(value / restored[3], expected, 1e-4));
    }
    for (depth, expected) in [(1.0, 0.0), (101.0, 1.0)] {
        let at_plane = apply(&[0.0, 0.0, depth, 1.0], &camera.view_to_clip);
        assert!(close(at_plane[2] / at_plane[3], expected, 1e-4));
    }
    let mut shifted = native;
    shifted.position_adjust[0] = 10.0;
    shifted.previous_position_adjust[0] = 9.0;
    let (admitted, shifted, history) = run(&shifted, Some(&raster), Some(&raster));
    assert!(admitted && history);
    let previous = apply(&clip, &shifted.clip_to_previous_clip);
    let expected_pixel = 0.75 + 99.2 * ((2.0 * (point[0] + 1.0) / z + 0.2) + 1.0) / 2.0;
    assert!(close(
        previous[0] / previous[3],
        2.0 * (expected_pixel - 1.0) / 99.0 - 1.0,
        1e-4
    ));

    let old = CameraRaster {
        xywh: [2.25, 0.5, 94.8, 78.1],
        crop: [2, 0, 97, 79],
    };
    assert!(matches!(
        run(&native, Some(&raster), Some(&old)),
        (true, _, false)
    ));
    assert!(matches!(
        run(&native, Some(&raster), None),
        (true, _, false)
    ));
    let mut empty = raster;
    empty.crop[2] = empty.crop[0];
    assert!(!run(&native, Some(&empty), Some(&raster)).0);
    let mut bad = raster;
    bad.xywh[2] = f32::NAN;
    assert!(!run(&native, Some(&bad), Some(&raster)).0);
    assert!(motion_to_uv(&bad).is_none());
    bad.xywh = raster.xywh;
    bad.xywh[3] = 0.0;
    assert!(motion_to_uv(&bad).is_none());
    let mut bad_camera = native;
    bad_camera.view = [0.0; 16];
    assert!(!run(&bad_camera, Some(&raster), Some(&raster)).0);
    bad_camera = native;
    bad_camera.projection[0] = f32::INFINITY;
    assert!(!run(&bad_camera, Some(&raster), Some(&raster)).0);
}

#[test]
fn motion_units_and_depth_bound() {
    let raster = CameraRaster {
        xywh: [0.75, 1.25, 99.2, 79.6],
        crop: [1, 1, 100, 81],
    };
    let motion = motion_to_uv(&raster).expect("motion scale");
    assert!(close((-4.0 / 99.2) * motion[0], 4.0 / 99.0, 1e-4));
    assert!(close((3.0 / 79.6) * motion[1], -3.0 / 80.0, 1e-4));

    let mut domain = xm::flatten(&xm::IDENTITY);
    let mut bound = ZERO_BOUND;
    assert!(depth_domain_forward(&domain, 0.01, &mut bound) && bound.minimum == 1.0);
    domain[3] = 0.1;
    domain[7] = -0.2;
    domain[11] = -0.3;
    assert!(depth_domain_forward(&domain, 0.01, &mut bound));
    let epsilon = 1.0 / (16_777_215.0 * f64::from(0.01_f32));
    let mut corners = f64::INFINITY;
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-epsilon, 1.0 + epsilon] {
                let w = x * f64::from(domain[3])
                    + y * f64::from(domain[7])
                    + z * f64::from(domain[11])
                    + f64::from(domain[15]);
                corners = corners.min(w);
            }
        }
    }
    assert!((bound.minimum - corners).abs() < 1e-12);
    domain[3] = 1.1;
    assert!(!depth_domain_forward(&domain, 0.01, &mut bound) && bound.minimum < 0.0);
    domain[3] = 0.0;
    domain[7] = 0.0;
    domain[11] = 0.0;
    domain[15] = 0.000_000_5;
    assert!(!depth_domain_forward(&domain, 0.01, &mut bound) && bound.minimum > 0.0);
    domain[15] = f32::INFINITY;
    assert!(!depth_domain_forward(&domain, 0.01, &mut bound));
}

fn perspective(x: f32, y: f32, near: f32, far: f32) -> Flat {
    let mut value = [0.0; 16];
    value[0] = x;
    value[5] = y;
    value[10] = far / (far - near);
    value[11] = 1.0;
    value[14] = -near * far / (far - near);
    value
}

#[test]
fn recorded_world_origin_and_eight_jitter_phases() {
    let (sin, cos) = 0.83_f32.sin_cos();
    let phases = [
        [0.0, -0.333_333_3],
        [-0.5, 0.333_333_3],
        [0.5, -0.777_777_8],
        [-0.75, -0.111_111_1],
        [0.25, 0.555_555_6],
        [-0.25, -0.555_555_6],
        [0.75, 0.111_111_1],
        [-0.875, 0.777_777_8],
    ];
    let raster = CameraRaster {
        xywh: [0.0, 0.0, 2560.0, 1600.0],
        crop: [0, 0, 2560, 1600],
    };
    let projection = perspective(1.32417, 2.11867, 15.0, 353468.0);
    let mut native = fixture_camera();
    native.view = [
        cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let vp = xm::flatten(&xm::multiply(
        &xm::load(&native.view),
        &xm::load(&projection),
    ));
    native.previous_view_projection_unjittered = vp;
    native.position_adjust = [-80300.7, 90804.2, 7973.14];
    native.previous_position_adjust = [
        native.position_adjust[0] - 0.25,
        native.position_adjust[1] + 0.125,
        native.position_adjust[2] - 0.0625,
    ];
    for [x, y] in phases {
        native.projection = projection;
        native.projection[8] += x / 2560.0;
        native.projection[9] += y / 1600.0;
        native.view_projection_unjittered = vp;
        let mut camera = unsafe { std::mem::zeroed() };
        let mut history = false;
        let mut backward = [0.0; 16];
        let mut build = |native: &NativeCamera| {
            let valid = camera_frame(
                native,
                2560,
                1600,
                &mut camera,
                &mut backward,
                &mut history,
                Some(&raster),
                Some(&raster),
            );
            (valid, camera, history)
        };
        let (valid, camera, history) = build(&native);
        assert!(valid && history);
        for (&value, expected) in camera.view_to_clip.iter().zip(projection) {
            assert!(close(value, expected, 1e-4));
        }
        assert!(close(camera.jitter_pixels[0], x / 2.0, 1e-4));
        assert!(close(camera.jitter_pixels[1], -y / 2.0, 1e-4));
        assert!(
            close(camera.near_plane, 15.0, 0.001) && (camera.far_plane - 353468.0).abs() < 1000.0
        );
        let clip = apply(&[2.0, 1.0, 100.0, 1.0], &camera.view_to_clip);
        let previous = apply(&clip, &camera.clip_to_previous_clip);
        let previous_x = 2.0 + 0.25 * cos + 0.0625 * sin;
        let previous_z = 100.0 - 0.25 * sin + 0.0625 * cos;
        assert!(close(
            previous[0] / previous[3],
            1.32417 * previous_x / previous_z,
            1e-4
        ));
        assert!(close(
            previous[1] / previous[3],
            2.11867 * (1.0 - 0.125) / previous_z,
            1e-4
        ));
        native.view_projection_unjittered[0] += 0.1;
        assert!(!build(&native).0);
    }
}

#[test]
fn first_person_composition_preserves_view_depth() {
    let mut near = perspective(2.3, 3.1, 1.0, 1000.0);
    let world = perspective(1.32417, 2.11867, 15.0, 353468.0);
    let mut bound = ZERO_BOUND;
    let mut composed = [0.0; 16];
    for jitter in [-0.000_341_797, 0.000_195_312_5] {
        near[8] = jitter;
        near[9] = 0.000_486_111;
        assert!(composed_view_depth(&near, &world, &mut composed));
        assert!(depth_domain_forward(&composed, 0.01, &mut bound));
        for z in [20.0, 200.0, 800.0] {
            let local_clip = apply(&[4.0, -3.0, z, 1.0], &near);
            let target = apply(&local_clip, &composed);
            let expected = (353468.0 / (353468.0 - 15.0)) * (1.0 - 15.0 / z);
            assert!(close(target[2] / target[3], expected, 0.000002));
            assert!(close(target[3], z, 0.001));
        }
    }

    // Reusing the recorded physical origins would put this near point behind the world camera.
    let delta = [
        -0.007_691_84 - (-80300.664),
        0.031_905_42 - 90804.234,
        120.485_94 - 7973.1445,
    ];
    let inverse = invert(&xm::load(&near)).expect("fixture projection is invertible");
    let translated = xm::multiply(&inverse, &xm::translation(delta[0], delta[1], delta[2]));
    let physical = xm::flatten(&xm::multiply(&translated, &xm::load(&world)));
    assert!(!depth_domain_forward(&physical, 0.01, &mut bound) && bound.minimum < 0.0);
    let point = apply(&apply(&[4.0, -3.0, 40.0, 1.0], &near), &physical);
    assert!(point[3] < 0.0 && close(point[3], 40.0 + delta[2], 0.01));

    assert!(!composed_view_depth(&[0.0; 16], &world, &mut composed));
    near[3] = 0.1;
    assert!(!composed_view_depth(&near, &world, &mut composed));
    near[3] = 0.0;
    near[0] = f32::NAN;
    assert!(!composed_view_depth(&near, &world, &mut composed));
}
