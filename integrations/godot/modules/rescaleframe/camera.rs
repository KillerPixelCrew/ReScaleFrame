// SPDX-License-Identifier: MIT
//! Projection and reprojection in Godot's column order, also the SDK's row-vector layout.

pub(crate) type Matrix = [f32; 16];

pub(crate) const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

pub(crate) fn multiply(left: &Matrix, right: &Matrix) -> Matrix {
    let mut result = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            for k in 0..4 {
                result[4 * column + row] += left[4 * k + row] * right[4 * column + k];
            }
        }
    }
    result
}

pub(crate) fn corrected_projection(native: &Matrix) -> Matrix {
    // Godot's reverse-Z, zero-to-one depth correction with the SDK's +Y-up convention.
    let mut correction = IDENTITY;
    correction[10] = -0.5;
    correction[14] = 0.5;
    multiply(&correction, native)
}

pub(crate) fn relative_view(previous_inverse: &Matrix, current: &Matrix) -> Matrix {
    let mut relative = IDENTITY;
    for column in 0..4 {
        for row in 0..3 {
            relative[4 * column + row] = previous_inverse[row] * current[4 * column]
                + previous_inverse[4 + row] * current[4 * column + 1]
                + previous_inverse[8 + row] * current[4 * column + 2];
        }
    }
    for row in 0..3 {
        relative[12 + row] += previous_inverse[12 + row];
    }
    relative
}
