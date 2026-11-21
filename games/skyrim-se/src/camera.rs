// SPDX-License-Identifier: MIT
//! Decode the copied Skyrim row-vector camera cache for the GameSDK.

mod render;

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

/// The private C++ wrapper lends the copied cache and three nonoverlapping outputs.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_skyrim_camera_frame(
    native: &NativeCamera,
    width: u32,
    height: u32,
    out: *mut rsf_camera_frame,
    previous_clip_to_clip: *mut Matrix,
    history_valid: *mut bool,
) -> bool {
    let Some((camera, history)) = decode(native, width, height) else {
        // The SDK record contains only integers and floats.
        unsafe {
            out.write(std::mem::zeroed());
            history_valid.write(false);
        }
        return false;
    };
    unsafe {
        out.write(camera);
        history_valid.write(history.is_some());
        previous_clip_to_clip.write(history.unwrap_or(IDENTITY));
    }
    true
}
