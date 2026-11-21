// SPDX-License-Identifier: MIT
// DirectXMath-derived operations Copyright (c) Microsoft Corporation.
// See the repository LICENSE for the MIT terms.
//! Scalar transcription of the DirectXMath matrix operations the Fallout 4 camera uses.
//!
//! The former camera.cpp included the Windows SDK 10.0.26100 DirectXMath 3.20 headers. The
//! adapter builds for x64 without `/arch`, so `_XM_SSE_INTRINSICS_` is the only path: no FMA3,
//! no SSE3 `hadd` and no SSE4 `dp`. Each SSE lane is one IEEE single operation, so performing
//! the same operations on the same lanes in the same order reproduces every result bit for bit.
//! The bodies below follow `XMMatrixMultiply`, `XMMatrixInverse` and `XMVector4Dot` line by line;
//! `shuffle` takes its indices in `_MM_SHUFFLE(fp3, fp2, fp1, fp0)` order.

pub(crate) type Vector = [f32; 4];
/// Four rows, the `XMMATRIX` layout. Points are row vectors.
pub(crate) type Matrix = [Vector; 4];

/// `_mm_shuffle_ps(a, b, _MM_SHUFFLE(fp3, fp2, fp1, fp0))`.
fn shuffle(a: Vector, b: Vector, [fp3, fp2, fp1, fp0]: [usize; 4]) -> Vector {
    [a[fp0 & 3], a[fp1 & 3], b[fp2 & 3], b[fp3 & 3]]
}

/// `XM_PERMUTE_PS(v, _MM_SHUFFLE(...))` without AVX.
fn permute(v: Vector, order: [usize; 4]) -> Vector {
    shuffle(v, v, order)
}

fn mul(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|lane| a[lane] * b[lane])
}

fn add(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|lane| a[lane] + b[lane])
}

fn sub(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|lane| a[lane] - b[lane])
}

fn div(a: Vector, b: Vector) -> Vector {
    std::array::from_fn(|lane| a[lane] / b[lane])
}

/// `XM_FNMADD_PS` without FMA3: `c - a * b`, rounded after the product and the difference.
fn fnmadd(a: Vector, b: Vector, c: Vector) -> Vector {
    sub(c, mul(a, b))
}

/// `XMLoadFloat4x4`.
pub(crate) fn load(values: &[f32; 16]) -> Matrix {
    std::array::from_fn(|row| std::array::from_fn(|column| values[row * 4 + column]))
}

/// `XMStoreFloat4x4`.
pub(super) fn flatten(matrix: &Matrix) -> [f32; 16] {
    std::array::from_fn(|index| matrix[index / 4][index % 4])
}

pub(super) const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `XMMatrixTranslation`.
pub(crate) fn translation(x: f32, y: f32, z: f32) -> Matrix {
    [IDENTITY[0], IDENTITY[1], IDENTITY[2], [x, y, z, 1.0]]
}

/// `XMMatrixMultiply(a, b)`, the SSE path: `(x*b0 + z*b2) + (y*b1 + w*b3)` per row.
pub(crate) fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    a.map(|row| {
        let x = mul(permute(row, [0, 0, 0, 0]), b[0]);
        let y = mul(permute(row, [1, 1, 1, 1]), b[1]);
        let z = mul(permute(row, [2, 2, 2, 2]), b[2]);
        let w = mul(permute(row, [3, 3, 3, 3]), b[3]);
        add(add(x, z), add(y, w))
    })
}

/// `XMVector4Dot`, the SSE2 path: `(p1 + p3) + (p0 + p2)` splatted.
fn dot4(v1: Vector, v2: Vector) -> Vector {
    let mut temp2 = v2;
    let mut temp = mul(v1, temp2);
    temp2 = shuffle(temp2, temp, [1, 0, 0, 0]);
    temp2 = add(temp2, temp);
    temp = shuffle(temp, temp2, [0, 3, 0, 0]);
    temp = add(temp, temp2);
    permute(temp, [2, 2, 2, 2])
}

/// `XMMatrixInverse(&determinant, m)`, the SSE path. Returns the result and the X lane of the
/// determinant. Like DirectXMath it never refuses: a zero determinant yields infinities or NaN.
pub(super) fn inverse(m: &Matrix) -> (Matrix, f32) {
    let temp1 = shuffle(m[0], m[1], [1, 0, 1, 0]);
    let temp3 = shuffle(m[0], m[1], [3, 2, 3, 2]);
    let temp2 = shuffle(m[2], m[3], [1, 0, 1, 0]);
    let temp4 = shuffle(m[2], m[3], [3, 2, 3, 2]);
    let mt = [
        shuffle(temp1, temp2, [2, 0, 2, 0]),
        shuffle(temp1, temp2, [3, 1, 3, 1]),
        shuffle(temp3, temp4, [2, 0, 2, 0]),
        shuffle(temp3, temp4, [3, 1, 3, 1]),
    ];

    let mut v00 = permute(mt[2], [1, 1, 0, 0]);
    let mut v10 = permute(mt[3], [3, 2, 3, 2]);
    let mut v01 = permute(mt[0], [1, 1, 0, 0]);
    let mut v11 = permute(mt[1], [3, 2, 3, 2]);
    let mut v02 = shuffle(mt[2], mt[0], [2, 0, 2, 0]);
    let mut v12 = shuffle(mt[3], mt[1], [3, 1, 3, 1]);

    let mut d0 = mul(v00, v10);
    let mut d1 = mul(v01, v11);
    let mut d2 = mul(v02, v12);

    v00 = permute(mt[2], [3, 2, 3, 2]);
    v10 = permute(mt[3], [1, 1, 0, 0]);
    v01 = permute(mt[0], [3, 2, 3, 2]);
    v11 = permute(mt[1], [1, 1, 0, 0]);
    v02 = shuffle(mt[2], mt[0], [3, 1, 3, 1]);
    v12 = shuffle(mt[3], mt[1], [2, 0, 2, 0]);

    d0 = fnmadd(v00, v10, d0);
    d1 = fnmadd(v01, v11, d1);
    d2 = fnmadd(v02, v12, d2);
    // V11 = D0Y,D0W,D2Y,D2Y
    v11 = shuffle(d0, d2, [1, 1, 3, 1]);
    v00 = permute(mt[1], [1, 0, 2, 1]);
    v10 = shuffle(v11, d0, [0, 3, 0, 2]);
    v01 = permute(mt[0], [0, 1, 0, 2]);
    v11 = shuffle(v11, d0, [2, 1, 2, 1]);
    // V13 = D1Y,D1W,D2W,D2W
    let mut v13 = shuffle(d1, d2, [3, 3, 3, 1]);
    v02 = permute(mt[3], [1, 0, 2, 1]);
    v12 = shuffle(v13, d1, [0, 3, 0, 2]);
    let mut v03 = permute(mt[2], [0, 1, 0, 2]);
    v13 = shuffle(v13, d1, [2, 1, 2, 1]);

    let mut c0 = mul(v00, v10);
    let mut c2 = mul(v01, v11);
    let mut c4 = mul(v02, v12);
    let mut c6 = mul(v03, v13);

    // V11 = D0X,D0Y,D2X,D2X
    v11 = shuffle(d0, d2, [0, 0, 1, 0]);
    v00 = permute(mt[1], [2, 1, 3, 2]);
    v10 = shuffle(d0, v11, [2, 1, 0, 3]);
    v01 = permute(mt[0], [1, 3, 2, 3]);
    v11 = shuffle(d0, v11, [0, 2, 1, 2]);
    // V13 = D1X,D1Y,D2Z,D2Z
    v13 = shuffle(d1, d2, [2, 2, 1, 0]);
    v02 = permute(mt[3], [2, 1, 3, 2]);
    v12 = shuffle(d1, v13, [2, 1, 0, 3]);
    v03 = permute(mt[2], [1, 3, 2, 3]);
    v13 = shuffle(d1, v13, [0, 2, 1, 2]);

    c0 = fnmadd(v00, v10, c0);
    c2 = fnmadd(v01, v11, c2);
    c4 = fnmadd(v02, v12, c4);
    c6 = fnmadd(v03, v13, c6);

    v00 = permute(mt[1], [0, 3, 0, 3]);
    // V10 = D0Z,D0Z,D2X,D2Y
    v10 = shuffle(d0, d2, [1, 0, 2, 2]);
    v10 = permute(v10, [0, 2, 3, 0]);
    v01 = permute(mt[0], [2, 0, 3, 1]);
    // V11 = D0X,D0W,D2X,D2Y
    v11 = shuffle(d0, d2, [1, 0, 3, 0]);
    v11 = permute(v11, [2, 1, 0, 3]);
    v02 = permute(mt[3], [0, 3, 0, 3]);
    // V12 = D1Z,D1Z,D2Z,D2W
    v12 = shuffle(d1, d2, [3, 2, 2, 2]);
    v12 = permute(v12, [0, 2, 3, 0]);
    v03 = permute(mt[2], [2, 0, 3, 1]);
    // V13 = D1X,D1W,D2Z,D2W
    v13 = shuffle(d1, d2, [3, 2, 3, 0]);
    v13 = permute(v13, [2, 1, 0, 3]);

    v00 = mul(v00, v10);
    v01 = mul(v01, v11);
    v02 = mul(v02, v12);
    v03 = mul(v03, v13);
    let c1 = sub(c0, v00);
    c0 = add(c0, v00);
    let c3 = add(c2, v01);
    c2 = sub(c2, v01);
    let c5 = sub(c4, v02);
    c4 = add(c4, v02);
    let c7 = add(c6, v03);
    c6 = sub(c6, v03);

    c0 = shuffle(c0, c1, [3, 1, 2, 0]);
    c2 = shuffle(c2, c3, [3, 1, 2, 0]);
    c4 = shuffle(c4, c5, [3, 1, 2, 0]);
    c6 = shuffle(c6, c7, [3, 1, 2, 0]);
    c0 = permute(c0, [3, 1, 2, 0]);
    c2 = permute(c2, [3, 1, 2, 0]);
    c4 = permute(c4, [3, 1, 2, 0]);
    c6 = permute(c6, [3, 1, 2, 0]);
    // Get the determinant
    let determinant = dot4(c0, mt[0]);
    let reciprocal = div([1.0; 4], determinant);
    (
        [
            mul(c0, reciprocal),
            mul(c2, reciprocal),
            mul(c4, reciprocal),
            mul(c6, reciprocal),
        ],
        determinant[0],
    )
}
