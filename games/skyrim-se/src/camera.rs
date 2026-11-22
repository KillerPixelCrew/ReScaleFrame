// SPDX-License-Identifier: MIT
//! Decode the copied Skyrim row-vector camera cache for the GameSDK.

pub(crate) mod render;

use rsf_game_sys::camera_math::inverse_4x4;
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};

type Matrix = [f32; 16];

#[repr(C)]
struct NativeCamera {
    view: Matrix,
    projection: Matrix,
    view_projection: Matrix,
    previous_view_projection: Matrix,
    origin: [f32; 3],
    previous_origin: [f32; 3],
    jitter_ndc: [f32; 2],
    near_plane: f32,
    far_plane: f32,
}

const _: () = {
    assert!(size_of::<NativeCamera>() == 296);
    assert!(std::mem::offset_of!(NativeCamera, origin) == 256);
    assert!(std::mem::offset_of!(NativeCamera, jitter_ndc) == 280);
};

const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    // Match the native conversion's double accumulation and float rounding per product.
    std::array::from_fn(|i| {
        let row = i / 4;
        let column = i % 4;
        let mut sum = 0.0;
        for k in 0..4 {
            sum += f64::from(a[row * 4 + k]) * f64::from(b[k * 4 + column]);
        }
        sum as f32
    })
}

fn inverse(source: &Matrix) -> Option<Matrix> {
    if source.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let result = inverse_4x4(&source.map(f64::from))?.map(|value| value as f32);
    result
        .iter()
        .all(|value| value.is_finite())
        .then_some(result)
}

fn close(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 0.0002 * 1.0_f32.max(a.abs()).max(b.abs())
}

fn decode(
    native: &NativeCamera,
    width: u32,
    height: u32,
) -> Option<(rsf_camera_frame, Option<Matrix>)> {
    let projection = &native.projection;
    if width == 0
        || height == 0
        || !native.near_plane.is_finite()
        || !native.far_plane.is_finite()
        || native.near_plane <= 0.0
        || native.far_plane <= native.near_plane
        || projection[0] <= 0.0
        || projection[5] <= 0.0
        || !close(projection[11], 1.0)
        || !close(projection[15], 0.0)
    {
        return None;
    }
    let q = native.far_plane / (native.far_plane - native.near_plane);
    if !close(projection[10], q) || !close(projection[14], -q * native.near_plane) {
        return None;
    }
    let product = multiply(&native.view, projection);
    if !product
        .iter()
        .zip(native.view_projection)
        .all(|(&a, b)| close(a, b))
    {
        return None;
    }
    let clip_to_view = inverse(projection)?;
    let inverse_vp = inverse(&native.view_projection)?;
    let mut translation = IDENTITY;
    for (x, &origin) in translation[12..15].iter_mut().zip(&native.origin) {
        if !origin.is_finite() {
            return None;
        }
        *x = -origin;
    }
    let world_to_view = multiply(&translation, &native.view);
    let view_to_world = inverse(&world_to_view)?;
    for (i, x) in translation[12..15].iter_mut().enumerate() {
        if !native.previous_origin[i].is_finite() {
            return None;
        }
        *x = native.origin[i] - native.previous_origin[i];
    }
    let adjusted = multiply(&inverse_vp, &translation);
    let clip_to_previous_clip = multiply(&adjusted, &native.previous_view_projection);
    let previous_clip_to_clip = inverse(&clip_to_previous_clip);

    // The native cache postmultiplies projection by an NDC translation.
    let jitter_pixels = [
        native.jitter_ndc[0] * width as f32 * 0.5,
        -native.jitter_ndc[1] * height as f32 * 0.5,
    ];
    if jitter_pixels.iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some((
        rsf_camera_frame {
            struct_size: size_of::<rsf_camera_frame>() as u32,
            abi_version: RSF_GAME_CAMERA_ABI_VERSION,
            view_to_clip: *projection,
            clip_to_view,
            view_to_world,
            world_to_view,
            clip_to_previous_clip: if previous_clip_to_clip.is_some() {
                clip_to_previous_clip
            } else {
                IDENTITY
            },
            jitter_pixels,
            previous_jitter_pixels: [0.0; 2],
            render_width: width,
            render_height: height,
            output_width: width,
            output_height: height,
            near_plane: native.near_plane,
            far_plane: native.far_plane,
            vertical_fov_radians: 2.0 * (1.0 / projection[5]).atan(),
            depth_inverted: 0,
            motion_jittered: 0,
            frame_time_seconds: 0.0,
        },
        previous_clip_to_clip,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transformed(point: &[f32; 4], matrix: &Matrix) -> [f32; 4] {
        std::array::from_fn(|column| {
            (0..4)
                .map(|row| point[row] * matrix[row * 4 + column])
                .sum()
        })
    }

    fn near(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }

    #[test]
    fn skyrim_camera_contract() {
        let projection = [
            2.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 1.01, 1.0, 0.0, 0.0, -1.01, 0.0,
        ];
        let mut native = NativeCamera {
            view: IDENTITY,
            projection,
            view_projection: projection,
            previous_view_projection: projection,
            origin: [10.0, 0.0, 0.0],
            previous_origin: [9.0, 0.0, 0.0],
            jitter_ndc: [-0.0005, 0.001],
            near_plane: 1.0,
            far_plane: 101.0,
        };

        let (frame, backward) = decode(&native, 1000, 500).expect("valid camera");
        let mut jittered = projection;
        // Native BuildCameraCache postmultiplies projection by clip translation.
        for row in jittered.chunks_exact_mut(4) {
            row[0] += row[3] * native.jitter_ndc[0];
            row[1] += row[3] * native.jitter_ndc[1];
        }
        let sample = [1.0, 2.0, 5.0, 1.0];
        let [x, y, _, w] = transformed(&sample, &projection);
        let [jx, jy, _, _] = transformed(&sample, &jittered);
        near(frame.jitter_pixels[0], (jx - x) / w * 500.0);
        near(frame.jitter_pixels[1], -(jy - y) / w * 250.0);
        near(
            transformed(&[10.0, 0.0, 5.0, 1.0], &frame.world_to_view)[0],
            0.0,
        );

        let clip = [0.0, 0.0, 4.04, 5.0];
        let previous = transformed(&clip, &frame.clip_to_previous_clip);
        near(previous[0] / previous[3], 0.4);
        let roundtrip = transformed(&previous, &backward.expect("valid camera history"));
        for (actual, expected) in roundtrip.into_iter().zip(clip) {
            near(actual, expected);
        }
        for (z, expected) in [(1.0, 0.0), (101.0, 1.0)] {
            let [_, _, clip_z, clip_w] = transformed(&[0.0, 0.0, z, 1.0], &frame.view_to_clip);
            near(clip_z / clip_w, expected);
        }

        native.previous_view_projection = [0.0; 16];
        assert!(
            decode(&native, 1000, 500)
                .expect("empty history resets")
                .1
                .is_none()
        );
        native.view_projection[0] += 1.0;
        assert!(decode(&native, 1000, 500).is_none());
        native.view_projection[0] -= 1.0;
        native.projection[0] = f32::NAN;
        assert!(decode(&native, 1000, 500).is_none());
    }
}
