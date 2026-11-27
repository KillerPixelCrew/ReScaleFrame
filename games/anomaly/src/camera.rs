// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};

pub(crate) type EngineMatrix = [f32; 16];
type Matrix = [f64; 16];

#[derive(Clone, Copy)]
pub(crate) struct Planes {
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) vertical_fov: f32,
}

// X-Ray's unjittered build_projection_HAT uses positive view Z and finite standard depth.
pub(crate) fn projection_planes(p: &EngineMatrix) -> Option<Planes> {
    let (q, q43, h) = (f64::from(p[10]), f64::from(p[14]), f64::from(p[5]));
    if p[11] != 1.0
        || p[15] != 0.0
        || !(q > 1.0 && q43 < 0.0 && h > 0.0)
        || !q.is_finite()
        || !q43.is_finite()
    {
        return None;
    }
    let near = -q43 / q;
    let far = q43 / (1.0 - q);
    (near > 0.0 && far > near && far.is_finite()).then(|| Planes {
        near: near as f32,
        far: far as f32,
        vertical_fov: (2.0 * (1.0 / h).atan()) as f32,
    })
}

pub(crate) fn frame(
    view: &EngineMatrix,
    projection: &EngineMatrix,
    previous_view: &EngineMatrix,
    previous_projection: &EngineMatrix,
    planes: Planes,
    [render_width, render_height, output_width, output_height]: [u32; 4],
    frame_time_seconds: f32,
) -> Option<(rsf_camera_frame, EngineMatrix)> {
    let view_d = view.map(f64::from);
    let projection_d = projection.map(f64::from);
    let clip_to_view = invert(projection_d)?;
    let view_to_world = invert(view_d)?;
    // Both X-Ray and the SDK use row vectors. A point goes through view, then projection.
    let clip_to_world = invert(multiply(view_d, projection_d))?;
    let previous_view_projection = multiply(
        previous_view.map(f64::from),
        previous_projection.map(f64::from),
    );
    let to_previous = multiply(clip_to_world, previous_view_projection);
    let from_previous = invert(to_previous)?;
    let narrow = |matrix: Matrix| matrix.map(|value| value as f32);
    Some((
        rsf_camera_frame {
            struct_size: size_of::<rsf_camera_frame>() as u32,
            abi_version: RSF_GAME_CAMERA_ABI_VERSION,
            view_to_clip: *projection,
            clip_to_view: narrow(clip_to_view),
            view_to_world: narrow(view_to_world),
            world_to_view: *view,
            clip_to_previous_clip: narrow(to_previous),
            jitter_pixels: [0.0; 2],
            previous_jitter_pixels: [0.0; 2],
            render_width,
            render_height,
            output_width,
            output_height,
            near_plane: planes.near,
            far_plane: planes.far,
            vertical_fov_radians: planes.vertical_fov,
            depth_inverted: 0,
            motion_jittered: 0,
            frame_time_seconds,
        },
        narrow(from_previous),
    ))
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Pose {
    pub(crate) position: [f64; 3],
    pub(crate) forward: [f64; 3],
    pub(crate) valid: bool,
}

pub(crate) fn camera_cut(previous: Pose, current: Pose, distance: f32, degrees: f32) -> bool {
    if !previous.valid || !current.valid {
        return true;
    }
    let distance_squared = current
        .position
        .into_iter()
        .zip(previous.position)
        .map(|(a, b)| (a - b) * (a - b))
        .sum::<f64>();
    let dot = current
        .forward
        .into_iter()
        .zip(previous.forward)
        .map(|(a, b)| a * b)
        .sum::<f64>();
    let angle = f64::from(degrees) * std::f64::consts::PI / 180.0;
    distance_squared.sqrt() > f64::from(distance) || dot < angle.cos()
}
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        (0..4).map(|k| a[row * 4 + k] * b[k * 4 + column]).sum()
    })
}

fn invert(matrix: Matrix) -> Option<Matrix> {
    rsf_game_sys::camera_math::inverse_4x4_with(
        &matrix,
        rsf_game_sys::camera_math::InversePolicy::Reciprocal,
    )
}

pub(crate) fn pose(view: &EngineMatrix) -> Pose {
    let Some(inverse) = invert(view.map(f64::from)) else {
        return Pose::default();
    };
    let position = [inverse[12], inverse[13], inverse[14]];
    let forward = [inverse[8], inverse[9], inverse[10]];
    let length = forward
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let valid = length > 1e-6;
    Pose {
        position,
        forward: if valid {
            forward.map(|value| value / length)
        } else {
            forward
        },
        valid,
    }
}
