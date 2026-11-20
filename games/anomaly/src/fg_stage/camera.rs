// SPDX-License-Identifier: MIT
use rsf_game_sys::{RSF_GAME_CAMERA_ABI_VERSION, rsf_camera_frame};

type Matrix = [f64; 16];

#[derive(Clone, Copy)]
pub(super) struct Camera {
    view: Matrix,
    projection: Matrix,
}

impl Camera {
    pub(super) fn new(view: [f32; 16], projection: [f32; 16]) -> Option<Self> {
        view.iter()
            .chain(&projection)
            .all(|value| value.is_finite())
            .then(|| Self {
                view: view.map(f64::from),
                projection: projection.map(f64::from),
            })
    }

    pub(super) fn is_cut_from(self, previous: Self) -> bool {
        let (Some((position, forward)), Some((past_position, past_forward))) =
            (pose(self.view), pose(previous.view))
        else {
            return true;
        };
        let distance = position
            .into_iter()
            .zip(past_position)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            .sqrt();
        let dot = forward
            .into_iter()
            .zip(past_forward)
            .map(|(a, b)| a * b)
            .sum::<f64>();
        // Same four-metre / 45-degree cut rule as SR.
        distance > 4.0 || dot < (std::f64::consts::PI / 4.0).cos()
    }

    pub(super) fn frame(
        self,
        previous: Self,
        [width, height]: [u32; 2],
        frame_time_seconds: f32,
    ) -> Option<(rsf_camera_frame, [f32; 16])> {
        // X-Ray's unjittered build_projection_HAT, standard finite depth. Row vectors use V * P.
        let p = self.projection;
        let (q, q43, h) = (p[10], p[14], p[5]);
        if p[11] != 1.0 || p[15] != 0.0 || q <= 1.0 || q43 >= 0.0 || h <= 0.0 {
            return None;
        }
        let near = -q43 / q;
        let far = q43 / (1.0 - q);
        if near <= 0.0 || far <= near || !far.is_finite() {
            return None;
        }
        let clip_to_view = invert(p)?;
        let view_to_world = invert(self.view)?;
        let clip_to_world = invert(multiply(self.view, p))?;
        let to_previous = multiply(clip_to_world, multiply(previous.view, previous.projection));
        let from_previous = invert(to_previous)?;
        let narrow = |matrix: Matrix| matrix.map(|value| value as f32);
        Some((
            rsf_camera_frame {
                struct_size: size_of::<rsf_camera_frame>() as u32,
                abi_version: RSF_GAME_CAMERA_ABI_VERSION,
                view_to_clip: narrow(p),
                clip_to_view: narrow(clip_to_view),
                view_to_world: narrow(view_to_world),
                world_to_view: narrow(self.view),
                clip_to_previous_clip: narrow(to_previous),
                jitter_pixels: [0.0; 2],
                previous_jitter_pixels: [0.0; 2],
                render_width: width,
                render_height: height,
                output_width: width,
                output_height: height,
                near_plane: near as f32,
                far_plane: far as f32,
                vertical_fov_radians: (2.0 * (1.0 / h).atan()) as f32,
                depth_inverted: 0,
                motion_jittered: 0,
                frame_time_seconds,
            },
            narrow(from_previous),
        ))
    }
}

fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        (0..4).map(|k| a[row * 4 + k] * b[k * 4 + column]).sum()
    })
}

fn invert(matrix: Matrix) -> Option<Matrix> {
    if !matrix.iter().all(|value| value.is_finite()) {
        return None;
    }
    let mut augmented: [[f64; 8]; 4] = std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            if column < 4 {
                matrix[row * 4 + column]
            } else {
                f64::from(column - 4 == row)
            }
        })
    });
    for column in 0..4 {
        let mut pivot = column;
        for row in column + 1..4 {
            if augmented[row][column].abs() > augmented[pivot][column].abs() {
                pivot = row;
            }
        }
        if augmented[pivot][column].abs() < 1e-300 {
            return None;
        }
        augmented.swap(column, pivot);
        let reciprocal = 1.0 / augmented[column][column];
        for value in &mut augmented[column] {
            *value *= reciprocal;
        }
        let normalized = augmented[column];
        for (row, values) in augmented.iter_mut().enumerate() {
            let factor = values[column];
            if row != column && factor != 0.0 {
                for (value, base) in values.iter_mut().zip(normalized) {
                    *value -= factor * base;
                }
            }
        }
    }
    Some(std::array::from_fn(|index| {
        augmented[index / 4][4 + index % 4]
    }))
}

fn pose(view: Matrix) -> Option<([f64; 3], [f64; 3])> {
    let inverse = invert(view)?;
    let position = [inverse[12], inverse[13], inverse[14]];
    let forward = [inverse[8], inverse[9], inverse[10]];
    let length = forward
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (length > 1e-6).then(|| (position, forward.map(|value| value / length)))
}
