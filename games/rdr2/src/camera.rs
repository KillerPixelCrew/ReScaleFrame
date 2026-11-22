// SPDX-License-Identifier: MIT
//! RAGE render camera decode for the 1491.50 worker view. The caller copies the active worker view
//! at the upscale boundary; no game pointer is retained. Matrices keep RAGE's negative-Z-forward
//! view convention and Z-up world coordinates.
#![allow(clippy::needless_range_loop)]

use crate::layout::view;
use rsf_game_sys::camera_math::{inverse_4x4 as inverse, multiply_4x4 as multiply};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};

type Matrix = [f64; 16];

#[cfg(any(test, feature = "camera-fixture"))]
mod fixture;

#[derive(Clone, Copy, Default)]
pub(crate) struct CameraInputs {
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) output_width: u32,
    pub(crate) output_height: u32,
    pub(crate) jitter: [f32; 2],
    pub(crate) previous_jitter: [f32; 2],
    pub(crate) frame_time_seconds: f32,
    pub(crate) reset: bool,
    pub(crate) motion_jittered: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CameraResult {
    pub(crate) camera: rsf_camera_frame,
    pub(crate) previous_clip_to_clip: [f32; 16],
    pub(crate) projection_jitter_matches: bool,
}

impl Default for CameraResult {
    fn default() -> Self {
        // Every field is plain numeric data.
        unsafe { std::mem::zeroed() }
    }
}

fn identity() -> Matrix {
    let mut result = [0.0; 16];
    for i in 0..4 {
        result[i * 4 + i] = 1.0;
    }
    result
}

fn scalar(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn matrix(bytes: &[u8], offset: usize) -> Option<Matrix> {
    let mut result = [0.0; 16];
    for (i, value) in result.iter_mut().enumerate() {
        let source = scalar(bytes, offset + i * 4);
        if !source.is_finite() {
            return None;
        }
        *value = f64::from(source);
    }
    Some(result)
}

fn store(source: &Matrix, destination: &mut [f32; 16]) -> bool {
    for (value, out) in source.iter().zip(destination.iter_mut()) {
        *out = *value as f32;
        if !out.is_finite() {
            return false;
        }
    }
    true
}

fn valid_extent(value: u32) -> bool {
    value != 0 && value <= 16384
}

/// Decodes a copied RAGE worker view; `None` refuses it.
pub(crate) fn decode(raw: &[u8], input: &CameraInputs) -> Option<CameraResult> {
    if raw.len() < view::BYTES
        || !valid_extent(input.render_width)
        || !valid_extent(input.render_height)
        || !valid_extent(input.output_width)
        || !valid_extent(input.output_height)
        || !input.frame_time_seconds.is_finite()
        || input.frame_time_seconds < 0.0
        || !input.jitter.iter().all(|value| value.is_finite())
        || !input.previous_jitter.iter().all(|value| value.is_finite())
        || raw[view::ACTIVE] != 1
    {
        return None;
    }
    let view_matrix = matrix(raw, view::VIEW)?;
    let mut projection = matrix(raw, view::PROJECTION)?;
    let world = matrix(raw, view::WORLD)?;
    let previous_view = matrix(raw, view::PREVIOUS_VIEW)?;
    let mut previous_projection = matrix(raw, view::PREVIOUS_PROJECTION)?;
    let near_plane = scalar(raw, view::NEAR);
    let far_plane = scalar(raw, view::FAR);
    let fov = scalar(raw, view::FOV_DEGREES);
    let jitter_x = scalar(raw, view::JITTER_X);
    let jitter_y = scalar(raw, view::JITTER_Y);
    let viewport_w = scalar(raw, view::VIEWPORT_WIDTH);
    let viewport_h = scalar(raw, view::VIEWPORT_HEIGHT);
    if !near_plane.is_finite()
        || !far_plane.is_finite()
        || !fov.is_finite()
        || near_plane <= 0.0
        || far_plane <= near_plane
        || fov <= 0.0
        || fov >= 180.0
        || !jitter_x.is_finite()
        || !jitter_y.is_finite()
        || !viewport_w.is_finite()
        || !viewport_h.is_finite()
        || viewport_w <= 0.0
        || viewport_h <= 0.0
        || projection[11] != -1.0
        || projection[15].abs() > 1e-6
    {
        return None;
    }
    // The NGX jitter is in render pixels; the projection carries it in clip units.
    let projection_jitter_matches =
        (-jitter_x * (viewport_w + 0.5).floor() / 2.0 - input.jitter[0]).abs() < 1e-4
            && (jitter_y * (viewport_h + 0.5).floor() / 2.0 - input.jitter[1]).abs() < 1e-4;
    // +0x180 is rebuilt with current jitter, not the preceding frame's jitter.
    projection[8] -= f64::from(jitter_x);
    projection[9] -= f64::from(jitter_y);
    previous_projection[8] -= f64::from(jitter_x);
    previous_projection[9] -= f64::from(jitter_y);
    let clip_to_view = inverse(&projection)?;
    let current_inverse = inverse(&multiply(&view_matrix, &projection))?;
    let (clip_to_previous, previous_to_clip) = if input.reset {
        (identity(), identity())
    } else {
        let clip_to_previous = multiply(
            &current_inverse,
            &multiply(&previous_view, &previous_projection),
        );
        (clip_to_previous, inverse(&clip_to_previous)?)
    };
    let mut decoded = CameraResult::default();
    let c = &mut decoded.camera;
    c.struct_size = size_of::<rsf_camera_frame>() as u32;
    c.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    if !store(&view_matrix, &mut c.world_to_view)
        || !store(&world, &mut c.view_to_world)
        || !store(&projection, &mut c.view_to_clip)
        || !store(&clip_to_view, &mut c.clip_to_view)
        || !store(&clip_to_previous, &mut c.clip_to_previous_clip)
        || !store(&previous_to_clip, &mut decoded.previous_clip_to_clip)
    {
        return None;
    }
    let c = &mut decoded.camera;
    c.jitter_pixels = input.jitter;
    c.previous_jitter_pixels = if input.reset {
        input.jitter
    } else {
        input.previous_jitter
    };
    c.render_width = input.render_width;
    c.render_height = input.render_height;
    c.output_width = input.output_width;
    c.output_height = input.output_height;
    c.near_plane = near_plane;
    c.far_plane = far_plane;
    // The nearest f32 to pi/180, as the native float literal rounded.
    c.vertical_fov_radians = fov * 0.017_453_292_f32;
    c.depth_inverted = 1;
    c.motion_jittered = u32::from(input.motion_jittered);
    c.frame_time_seconds = input.frame_time_seconds;
    decoded.projection_jitter_matches = projection_jitter_matches;
    Some(decoded)
}

#[cfg(test)]
mod tests;
