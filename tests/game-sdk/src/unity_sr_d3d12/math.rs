// SPDX-License-Identifier: MIT
use super::{HEIGHT, Result, WIDTH, check};
use rsf_game_sys::{
    camera_math::{finite_matrix_close, inverse_4x4, multiply_4x4},
    rsf_game_render_pass,
};
use rsf_graphics::dxgi_format::half_to_f32;

pub(super) const NEAR: f32 = 0.1;
pub(super) const FAR: f32 = 1000.0;
pub(super) const VERTICAL_FOV: f32 = std::f32::consts::FRAC_PI_3;
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Direct3D row-vector projection that XMMatrixPerspectiveFovRH builds when the physical far and near
/// planes are passed in reversed order, with depth 1 at the near plane and 0 at the far plane.
fn projection(fov: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let (sin, cos) = (fov * 0.5).sin_cos();
    let height = cos / sin;
    let depth = near / (far - near);
    [
        height / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        height,
        0.0,
        0.0,
        0.0,
        0.0,
        depth,
        -1.0,
        0.0,
        0.0,
        depth * far,
        0.0,
    ]
}

pub(super) fn set_matrices(pass: &mut rsf_game_render_pass) -> Result<()> {
    let camera = &mut pass.camera;
    let view_to_clip = projection(
        camera.vertical_fov_radians,
        camera.output_width as f32 / camera.output_height as f32,
        camera.near_plane,
        camera.far_plane,
    );
    let clip_to_view = inverse_4x4(&view_to_clip.map(f64::from))
        .ok_or("fixture projection is singular")?
        .map(|value| value as f32);
    camera.view_to_clip = view_to_clip;
    camera.clip_to_view = clip_to_view;
    camera.view_to_world = IDENTITY;
    camera.world_to_view = IDENTITY;
    camera.clip_to_previous_clip = IDENTITY;
    pass.previous_clip_to_clip = IDENTITY;
    Ok(())
}

pub(super) fn check_portable_math() -> Result<()> {
    let probes = [
        (0x0000, 0.0_f32),
        (0x8000, -0.0),
        (0x0001, 1.0 / 16_777_216.0),
        (0x03ff, 1023.0 / 16_777_216.0),
        (0x0400, 1.0 / 16_384.0),
        (0x3400, 0.25),
        (0x3800, 0.5),
        (0x3a00, 0.75),
        (0x3c00, 1.0),
        (0xbc00, -1.0),
        (0x7bff, 65_504.0),
        (0x7c00, f32::INFINITY),
        (0xfc00, f32::NEG_INFINITY),
    ];
    check(
        probes
            .into_iter()
            .all(|(bits, expected)| half_to_f32(bits).to_bits() == expected.to_bits())
            && half_to_f32(0x7e00).is_nan(),
        "binary16 conversion differs from reference values",
    )?;

    let mut pass: rsf_game_render_pass = Default::default();
    pass.camera.output_width = WIDTH;
    pass.camera.output_height = HEIGHT;
    pass.camera.vertical_fov_radians = VERTICAL_FOV;
    pass.camera.near_plane = NEAR;
    pass.camera.far_plane = FAR;
    set_matrices(&mut pass)?;
    let projection = &pass.camera.view_to_clip;
    let inverse = &pass.camera.clip_to_view;
    // Derive the expected clip coordinates from the fixture geometry. The 60° frustum has
    // slope 1/sqrt(3), independent of the trigonometric matrix construction and inverse solver.
    let physical_aspect = 16.0_f32 / 9.0;
    let up = 1.0 / 3.0_f32.sqrt();
    let right = up * physical_aspect;
    let project_point = |point: [f32; 4]| -> [f32; 4] {
        std::array::from_fn(|column| {
            (0..4)
                .map(|row| point[row] * projection[row * 4 + column])
                .sum()
        })
    };
    let samples = [
        ([NEAR * right, NEAR * up, -NEAR, 1.0], [NEAR; 4]),
        ([-FAR * right, -FAR * up, -FAR, 1.0], [-FAR, -FAR, 0.0, FAR]),
    ];
    check(
        samples.into_iter().all(|(point, expected)| {
            project_point(point)
                .into_iter()
                .zip(expected)
                .all(|(actual, expected)| {
                    (actual - expected).abs() <= 2.0e-6 * expected.abs().max(1.0)
                })
        }),
        "projection differs from independent frustum clip coordinates",
    )?;
    check(
        finite_matrix_close(&multiply_4x4(projection, inverse), &IDENTITY, 2.0e-6)
            && finite_matrix_close(&multiply_4x4(inverse, projection), &IDENTITY, 2.0e-6)
            && pass.camera.view_to_world == IDENTITY
            && pass.camera.world_to_view == IDENTITY
            && pass.camera.clip_to_previous_clip == IDENTITY
            && pass.previous_clip_to_clip == IDENTITY,
        "camera inverse or identity matrices differ from reference",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn binary16_and_reverse_z_camera_match_the_references() {
        super::check_portable_math().unwrap();
    }
}
