// SPDX-License-Identifier: MIT
//! Fallout 4 camera and raster/depth math: the copied row-vector camera cache to the GameSDK
//! camera frame, the integer crop affine, native motion units, and depth-domain admission.
//!
//! Matrix products and inverses go through `xm`, which repeats the DirectXMath SSE2 operations
//! the former camera.cpp compiled to, so the 1e-8 determinant refusal and every stored value see
//! the same single-precision rounding. The GameSDK `inverse_4x4` is not reused: it pivots in f64
//! with a 1e-12 pivot floor, which moves both the refusal boundary and the results.
//! All scalar work keeps the C++ types: f32 for the camera and raster, f64 for the depth bound.

pub(crate) use rsf_game_sys::xm;

pub(crate) use crate::sys::{CameraRaster, NativeCamera};
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};
use std::f32::consts::PI;
use xm::Matrix;

type Flat = [f32; 16];

/// `CameraDepthBound`, read by depth_domains.rs and the camera fixture.
#[repr(C)]
pub(crate) struct CameraDepthBound {
    pub(crate) coefficients: [f64; 4],
    pub(crate) minimum: f64,
    pub(crate) range: f64,
}

const _: () = {
    use std::mem::offset_of;
    assert!(size_of::<NativeCamera>() == 288);
    assert!(offset_of!(NativeCamera, projection) == 64);
    assert!(offset_of!(NativeCamera, view_projection_unjittered) == 128);
    assert!(offset_of!(NativeCamera, previous_view_projection_unjittered) == 192);
    assert!(offset_of!(NativeCamera, position_adjust) == 256);
    assert!(offset_of!(NativeCamera, previous_position_adjust) == 268);
    assert!(offset_of!(NativeCamera, jitter_ndc) == 280);
    assert!(size_of::<CameraRaster>() == 32);
    assert!(offset_of!(CameraRaster, crop) == 16);
    assert!(size_of::<CameraDepthBound>() == 48);
    assert!(offset_of!(CameraDepthBound, minimum) == 32);
    assert!(offset_of!(CameraDepthBound, range) == 40);
};

#[cfg(test)]
const ZERO_BOUND: CameraDepthBound = CameraDepthBound {
    coefficients: [0.0; 4],
    minimum: 0.0,
    range: 0.0,
};

// Ordered comparisons that are false for NaN, spelled as calls so `!above(x, 0.0)` keeps the
// `!(x>0)` meaning.
pub(crate) fn above(value: f32, bound: f32) -> bool {
    value > bound
}

fn below(value: f32, bound: f32) -> bool {
    value < bound
}

/// `XMStoreFloat4x4` into `output` only when every element is finite.
pub(crate) fn store(matrix: &Matrix, output: &mut Flat) -> bool {
    let values = xm::flatten(matrix);
    if values.iter().all(|value| value.is_finite()) {
        *output = values;
        true
    } else {
        false
    }
}

/// `XMMatrixInverse` admitted only for a finite determinant above 1e-8 in magnitude and a
/// finite result.
pub(crate) fn invert(matrix: &Matrix) -> Option<Matrix> {
    let (result, determinant) = xm::inverse(matrix);
    let mut checked = [0.0; 16];
    (determinant.is_finite() && determinant.abs() > 1e-8 && store(&result, &mut checked))
        .then_some(result)
}

/// Relative agreement with a unit floor.
pub(crate) fn near(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 0.0005 * 1.0_f32.max(a.abs()).max(b.abs())
}

/// Native NDC to integer crop NDC affine, row-vector convention.
pub(crate) fn raster_transform(raster: &CameraRaster) -> Option<Flat> {
    let [left, top, right, bottom] = raster.crop;
    if left < 0 || top < 0 || right <= left || bottom <= top {
        return None;
    }
    if raster.xywh.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let [x, y, width, height] = raster.xywh;
    if x < 0.0 || y < 0.0 || !above(width, 0.0) || !above(height, 0.0) {
        return None;
    }
    // left >= 0 and right > left, so neither extent overflows.
    let crop_width = (right - left) as f32;
    let crop_height = (bottom - top) as f32;
    let sx = width / crop_width;
    let sy = height / crop_height;
    let ox = (2.0 * (x - left as f32) + width) / crop_width - 1.0;
    let oy = 1.0 - (2.0 * (y - top as f32) + height) / crop_height;
    let mut output = [0.0; 16];
    let affine = [
        [sx, 0.0, 0.0, 0.0],
        [0.0, sy, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [ox, oy, 0.0, 1.0],
    ];
    store(&affine, &mut output).then_some(output)
}

/// D24 source clip-box forward-W admission, with one endpoint quantization step.
pub(crate) fn depth_domain_forward(m: &Flat, range: f32, output: &mut CameraDepthBound) -> bool {
    if !range.is_finite() || !above(range, 0.0) {
        return false;
    }
    if m.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let epsilon = 1.0 / (16_777_215.0 * f64::from(range));
    output.coefficients = [m[3], m[7], m[11], m[15]].map(f64::from);
    output.range = f64::from(range);
    let quantized_near = -epsilon * f64::from(m[11]);
    let quantized_far = (1.0 + epsilon) * f64::from(m[11]);
    // std::min(a, b) returns b only when b < a.
    let lower = if quantized_far < quantized_near {
        quantized_far
    } else {
        quantized_near
    };
    output.minimum = f64::from(m[15]) - f64::from(m[3]).abs() - f64::from(m[7]).abs() + lower;
    output.minimum.is_finite() && output.minimum > 1e-6
}

/// The native forward perspective projection form the composed convention requires.
fn forward_perspective(p: &Flat) -> bool {
    p.iter().all(|value| value.is_finite())
        && above(p[0], 0.0)
        && above(p[5], 0.0)
        && above(p[10], 1.0)
        && below(p[14], 0.0)
        && p[11] == 1.0
        && p[15] == 0.0
        && [1, 2, 3, 4, 6, 7, 12, 13]
            .iter()
            .all(|&index| p[index] == 0.0)
}

/// Composed first-person view-Z convention, not a shared-world point transform.
pub(crate) fn composed_view_depth(source: &Flat, target: &Flat, output: &mut Flat) -> bool {
    if !forward_perspective(source) || !forward_perspective(target) {
        return false;
    }
    let target = xm::load(target);
    let Some(source_inverse) = invert(&xm::load(source)) else {
        return false;
    };
    if invert(&target).is_none() {
        return false;
    }
    store(&xm::multiply(&source_inverse, &target), output)
}

struct FrameOutputs<'a> {
    camera: &'a mut rsf_camera_frame,
    previous_clip_to_clip: &'a mut Flat,
    history_valid: &'a mut bool,
}

impl CameraRaster {
    /// Compare the viewport's float bits, including the sign of zero.
    pub(crate) fn same_bits(&self, other: &Self) -> bool {
        self.xywh.map(f32::to_bits) == other.xywh.map(f32::to_bits) && self.crop == other.crop
    }
}

pub(crate) fn projection_jitter(
    jittered: &Flat,
    stable: &Flat,
    [width, height]: [f32; 2],
) -> Option<[f32; 2]> {
    if (0..16).any(|i| i != 8 && i != 9 && !near(jittered[i], stable[i])) {
        return None;
    }
    Some([
        (jittered[8] - stable[8]) * width * 0.5,
        -(jittered[9] - stable[9]) * height * 0.5,
    ])
}

/// `outputs` arrive zeroed with history invalid. A refusal may leave fields already written,
/// callers discard the frame on refusal.
fn frame(
    native: &NativeCamera,
    width: u32,
    height: u32,
    current: Option<&CameraRaster>,
    previous: Option<&CameraRaster>,
    outputs: FrameOutputs<'_>,
) -> bool {
    let FrameOutputs {
        camera: output,
        previous_clip_to_clip,
        history_valid,
    } = outputs;
    if native
        .position_adjust
        .iter()
        .chain(&native.previous_position_adjust)
        .chain(&native.jitter_ndc)
        .any(|value| !value.is_finite())
    {
        return false;
    }
    let view = xm::load(&native.view);
    let mut vp = xm::load(&native.view_projection_unjittered);
    let Some(inverse_view) = invert(&view) else {
        return false;
    };
    let Some(mut inverse_vp) = invert(&vp) else {
        return false;
    };
    // Native row-vector view*projection is cached independently before jitter.
    let mut projection = xm::multiply(&inverse_view, &vp);
    let mut p = [0.0; 16];
    if !store(&projection, &mut p) {
        return false;
    }
    let Some(mut inverse_projection) = invert(&projection) else {
        return false;
    };
    if !above(p[0], 0.0)
        || !above(p[5], 0.0)
        || !near(p[11], 1.0)
        || !near(p[15], 0.0)
        || !above(p[10], 1.0)
        || !below(p[14], 0.0)
    {
        return false;
    }
    let near_plane = -p[14] / p[10];
    let far_plane = -p[14] / (p[10] - 1.0);
    if !near_plane.is_finite()
        || !far_plane.is_finite()
        || !above(near_plane, 0.0)
        || !above(far_plane, near_plane)
    {
        return false;
    }
    // The native builder postmultiplies by an NDC translation. Its actual cached projection,
    // rather than a later cleared UI jitter global, supplies displacement.

    let mut raster_matrix = [0.0; 16];
    let mut previous_raster_matrix = [0.0; 16];
    if let Some(current) = current {
        // int32 differences compared against int32_t(width); wrap the same way.
        if current.crop[2].wrapping_sub(current.crop[0]) != width as i32
            || current.crop[3].wrapping_sub(current.crop[1]) != height as i32
        {
            return false;
        }
        let Some(affine) = raster_transform(current) else {
            return false;
        };
        raster_matrix = affine;
    }
    if let Some(previous) = previous {
        let Some(affine) = raster_transform(previous) else {
            return false;
        };
        previous_raster_matrix = affine;
    }
    let jitter_width = current.map_or(width as f32, |raster| raster.xywh[2]);
    let jitter_height = current.map_or(height as f32, |raster| raster.xywh[3]);
    let Some(jitter) = projection_jitter(&native.projection, &p, [jitter_width, jitter_height])
    else {
        return false;
    };
    output.jitter_pixels = jitter;
    if output
        .jitter_pixels
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 2.0)
    {
        return false;
    }
    if current.is_some() {
        let raster = xm::load(&raster_matrix);
        projection = xm::multiply(&projection, &raster);
        vp = xm::multiply(&vp, &raster);
        let Some(inverse) = invert(&projection) else {
            return false;
        };
        inverse_projection = inverse;
        let Some(inverse) = invert(&vp) else {
            return false;
        };
        inverse_vp = inverse;
        if !store(&projection, &mut p) {
            return false;
        }
    }
    let [x, y, z] = native.position_adjust;
    let world_view = xm::multiply(&xm::translation(-x, -y, -z), &view);
    let Some(view_world) = invert(&world_view) else {
        return false;
    };
    if !store(&world_view, &mut output.world_to_view)
        || !store(&view_world, &mut output.view_to_world)
        || !store(&projection, &mut output.view_to_clip)
        || !store(&inverse_projection, &mut output.clip_to_view)
    {
        return false;
    }
    let [px, py, pz] = native.previous_position_adjust;
    let delta = xm::translation(x - px, y - py, z - pz);
    let mut previous_vp = xm::load(&native.previous_view_projection_unjittered);
    if previous.is_some() {
        previous_vp = xm::multiply(&previous_vp, &xm::load(&previous_raster_matrix));
    }
    let reproject = xm::multiply(&xm::multiply(&inverse_vp, &delta), &previous_vp);
    let steady = match (current, previous) {
        (None, _) => true,
        (Some(current), Some(previous)) => current.same_bits(previous),
        (Some(_), None) => false,
    };
    let history = steady
        && invert(&reproject).is_some_and(|previous_to_current| {
            store(&reproject, &mut output.clip_to_previous_clip)
                && store(&previous_to_current, previous_clip_to_clip)
        });
    *history_valid = history;
    if !history {
        store(&xm::IDENTITY, &mut output.clip_to_previous_clip);
        store(&xm::IDENTITY, previous_clip_to_clip);
    }
    output.struct_size = size_of::<rsf_camera_frame>() as u32;
    output.abi_version = RSF_GAME_CAMERA_ABI_VERSION;
    output.render_width = width;
    output.output_width = width;
    output.render_height = height;
    output.output_height = height;
    output.near_plane = near_plane;
    output.far_plane = far_plane;
    output.vertical_fov_radians = ((1.0 - p[9]) / p[5]).atan() - ((-1.0 - p[9]) / p[5]).atan();
    output.depth_inverted = 0;
    output.motion_jittered = 0;
    // 3.14159265f rounds to the same single as PI.
    let fov = output.vertical_fov_radians;
    fov.is_finite() && fov > 0.0 && fov < PI
}

/// Clear the camera frame and mark history invalid before sampling.
#[allow(clippy::too_many_arguments)] // camera inputs, rasters and output records
pub(crate) fn camera_frame(
    native: &NativeCamera,
    width: u32,
    height: u32,
    output: &mut rsf_camera_frame,
    previous_clip_to_clip: &mut Flat,
    history_valid: &mut bool,
    current: Option<&CameraRaster>,
    previous: Option<&CameraRaster>,
) -> bool {
    // SAFETY: the SDK record holds only integers and floats.
    *output = unsafe { std::mem::zeroed() };
    *history_valid = false;
    if width == 0 || height == 0 {
        return false;
    }
    frame(
        native,
        width,
        height,
        current,
        previous,
        FrameOutputs {
            camera: output,
            previous_clip_to_clip,
            history_valid,
        },
    )
}

/// `camera_motion_to_uv`: native previous-minus-current UV to current-minus-previous
/// crop UV.
pub(crate) fn motion_to_uv(raster: &CameraRaster) -> Option<[f32; 2]> {
    raster_transform(raster).map(|affine| [-affine[0], -affine[5]])
}

#[cfg(test)]
#[path = "camera/tests.rs"]
mod tests;
