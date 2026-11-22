// SPDX-License-Identifier: MIT
//! Column-vector matrices (row-major storage, as the motion Mat4) and the skinned octahedron mover of the far
//! and tessellation-chain fixtures.
use super::super::Mat4;
use super::harness::{Gpu, Outcome, Signature};
use windows::Win32::Graphics::Direct3D11::ID3D11Buffer;

pub(super) type Vec3 = [f64; 3];

pub(super) fn rotation_x(angle: f64) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    let (sin, cos) = angle.sin_cos();
    m.0[5] = cos;
    m.0[6] = -sin;
    m.0[9] = sin;
    m.0[10] = cos;
    m
}

pub(super) fn rotation_y(angle: f64) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    let (sin, cos) = angle.sin_cos();
    m.0[0] = cos;
    m.0[2] = sin;
    m.0[8] = -sin;
    m.0[10] = cos;
    m
}

pub(super) fn translation(x: f64, y: f64, z: f64) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    m.0[3] = x;
    m.0[7] = y;
    m.0[11] = z;
    m
}

/// D3D left-handed perspective (column-vector form of D3DXMatrixPerspectiveFovLH), as X-Ray builds it.
pub(super) fn perspective(fov_y: f64, near: f64, far: f64) -> Mat4 {
    let mut m = Mat4::default();
    let scale = 1. / (fov_y / 2.).tan();
    m.0[0] = scale;
    m.0[5] = scale;
    m.0[10] = far / (far - near);
    m.0[11] = -near * far / (far - near);
    m.0[14] = 1.;
    m
}

pub(super) fn apply(m: &Mat4, v: [f64; 4]) -> [f64; 4] {
    std::array::from_fn(|r| (0..4).map(|c| m.0[r * 4 + c] * v[c]).sum())
}

/// Inverse of a perspective or affine 4x4 by Gauss-Jordan, in double.
pub(super) fn invert(m: &Mat4) -> Mat4 {
    let mut a = [[0f64; 8]; 4];
    for (r, row) in a.iter_mut().enumerate() {
        row[..4].copy_from_slice(&m.0[r * 4..r * 4 + 4]);
        row[4 + r] = 1.;
    }
    for c in 0..4 {
        let mut pivot = c;
        for r in c + 1..4 {
            if a[r][c].abs() > a[pivot][c].abs() {
                pivot = r;
            }
        }
        a.swap(c, pivot);
        let d = a[c][c];
        for value in &mut a[c] {
            *value /= d;
        }
        for r in 0..4 {
            if r != c {
                let f = a[r][c];
                let pivot = a[c];
                for (value, p) in a[r].iter_mut().zip(pivot) {
                    *value -= f * p;
                }
            }
        }
    }
    let mut out = Mat4::default();
    for (r, row) in a.iter().enumerate() {
        out.0[r * 4..r * 4 + 4].copy_from_slice(&row[4..]);
    }
    out
}

fn normalize(v: Vec3) -> Vec3 {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / length)
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Previous clip-space UV motion (current minus previous) of a pixel centre.
pub(super) fn uv_motion(u: f64, v: f64, previous: [f64; 4]) -> (f64, f64) {
    (
        u - (previous[0] / previous[3] * 0.5 + 0.5),
        v - (previous[1] / previous[3] * -0.5 + 0.5),
    )
}

/// The current palette: every one of the 78 affine bones identity (rows).
pub(super) fn identity_bones() -> Vec<f32> {
    let mut bones = vec![0f32; 234 * 4];
    for bone in 0..78 {
        for row in 0..3 {
            bones[(bone * 3 + row) * 4 + row] = 1.;
        }
    }
    bones
}

pub(super) fn floats(m: &Mat4) -> Vec<f32> {
    m.0.iter().map(|&value| value as f32).collect()
}

/// An octahedron with outward vertex normals as 8 triangles (24 vertices, or 8 patches of 3 control points),
/// laid out against the STOCK VS signature as the engine builds its input layout.
pub(super) fn octahedron(
    gpu: &Gpu,
    signature: &Signature,
    centre: Vec3,
    radius: Vec3,
    skin: i32,
) -> Outcome<(ID3D11Buffer, u32)> {
    let axes: [Vec3; 6] = [
        [1., 0., 0.],
        [-1., 0., 0.],
        [0., 1., 0.],
        [0., -1., 0.],
        [0., 0., 1.],
        [0., 0., -1.],
    ];
    let mut corners = Vec::new();
    for sx in 0..2 {
        for sy in 0..2 {
            for sz in 0..2 {
                corners.extend([sx, 2 + sy, 4 + sz]);
            }
        }
    }
    let inputs = signature.inputs.len();
    let mut data = vec![0f32; corners.len() * inputs * 4];
    // skin.h unpacks D3DCOLOR normals: the shader swizzles .zyx and maps [0, 1] to [-1, 1].
    let packed = |n: Vec3| {
        [
            ((n[2] + 1.) / 2.) as f32,
            ((n[1] + 1.) / 2.) as f32,
            ((n[0] + 1.) / 2.) as f32,
        ]
    };
    for (v, &corner) in corners.iter().enumerate() {
        let n = axes[corner];
        let position: Vec3 = std::array::from_fn(|c| centre[c] + radius[c] * n[c]);
        let up = if n[1].abs() > 0.9 {
            [1., 0., 0.]
        } else {
            [0., 1., 0.]
        };
        let t = normalize(cross(up, n));
        let b = cross(n, t);
        for i in 0..inputs {
            let out = &mut data[(v * inputs + i) * 4..][..4];
            let row = &signature.inputs[i];
            match signature.semantic(i) {
                b"POSITION" => out.copy_from_slice(&[
                    position[0] as f32,
                    position[1] as f32,
                    position[2] as f32,
                    1.,
                ]),
                b"NORMAL" => {
                    let [x, y, z] = packed(n);
                    out.copy_from_slice(&[x, y, z, if skin == 2 { 0.25 } else { 0. }]);
                }
                b"TANGENT" => {
                    let [x, y, z] = packed(t);
                    out.copy_from_slice(&[x, y, z, 0.]);
                }
                b"BINORMAL" => {
                    let [x, y, z] = packed(b);
                    out.copy_from_slice(&[x, y, z, 0.]);
                }
                // u, v from the normal; SKIN_2 bone rows in z/w (palette rows 0 and 3 carry the same matrix).
                b"TEXCOORD" if row.SemanticIndex == 0 => out.copy_from_slice(&[
                    (0.5 + 0.45 * n[0] + 0.2 * n[2]) as f32,
                    (0.5 + 0.45 * n[1] - 0.2 * n[2]) as f32,
                    0.,
                    3.,
                ]),
                _ => {}
            }
        }
    }
    Ok((gpu.vertex_buffer(&data)?, corners.len() as u32))
}
