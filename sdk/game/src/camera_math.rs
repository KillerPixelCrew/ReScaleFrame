// SPDX-License-Identifier: MIT
//! Matrix operations shared by the native camera adapters.

use core::ops::{AddAssign, Mul};

/// Multiplies row-major 4×4 matrices in ascending inner-index order.
/// The caller's scalar type determines the product and accumulator precision.
pub fn multiply_4x4<T>(a: &[T; 16], b: &[T; 16]) -> [T; 16]
where
    T: Copy + Default + AddAssign + Mul<Output = T>,
{
    core::array::from_fn(|index| {
        let row = index / 4;
        let column = index % 4;
        let mut value = T::default();
        for inner in 0..4 {
            value += a[row * 4 + inner] * b[inner * 4 + column];
        }
        value
    })
}

#[cfg(test)]
mod product_tests {
    use super::*;

    #[test]
    fn products_preserve_operand_order_and_scalar_precision() {
        let scale = [
            2.0_f32, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let translation = [
            1.0_f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, -1.0, 3.0, 1.0,
        ];
        assert_eq!(
            multiply_4x4(&scale, &translation)[12..],
            [2.0, -1.0, 3.0, 1.0]
        );
        assert_eq!(
            multiply_4x4(&translation, &scale)[12..],
            [4.0, -3.0, 12.0, 1.0]
        );
        let mut subtle = translation.map(f64::from);
        subtle[12] += f64::EPSILON * 2.0;
        assert_eq!(
            multiply_4x4(&subtle, &scale.map(f64::from))[12],
            4.0 + f64::EPSILON * 4.0
        );
    }
}

/// Inverts a row-major 4×4 matrix with partial pivoting and a 1e-12 pivot floor.
pub fn inverse_4x4(input: &[f64; 16]) -> Option<[f64; 16]> {
    inverse_4x4_with(input, InversePolicy::Division)
}

/// Numerical conventions retained by callers of the partial-pivot solver.
#[derive(Clone, Copy)]
pub enum InversePolicy {
    /// A 1e-12 floor, finite pivot checks, and division during normalization.
    Division,
    /// A 1e-300 floor, finite inputs, reciprocal multiplication and zero-factor skipping.
    Reciprocal,
}

pub fn inverse_4x4_with(input: &[f64; 16], policy: InversePolicy) -> Option<[f64; 16]> {
    let reciprocal = matches!(policy, InversePolicy::Reciprocal);
    if reciprocal && input.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut rows = [[0.0; 8]; 4];
    for (index, row) in rows.iter_mut().enumerate() {
        row[..4].copy_from_slice(&input[index * 4..index * 4 + 4]);
        row[index + 4] = 1.0;
    }
    for column in 0..4 {
        let mut pivot = column;
        for index in column + 1..4 {
            if rows[index][column].abs() > rows[pivot][column].abs() {
                pivot = index;
            }
        }
        let pivot_value = rows[pivot][column];
        let floor = if reciprocal { 1e-300 } else { 1e-12 };
        if pivot_value.abs() < floor || (!reciprocal && !pivot_value.is_finite()) {
            return None;
        }
        rows.swap(column, pivot);
        let divisor = rows[column][column];
        if reciprocal {
            let scale = 1.0 / divisor;
            for value in &mut rows[column] {
                *value *= scale;
            }
        } else {
            for value in &mut rows[column] {
                *value /= divisor;
            }
        }
        let normalized = rows[column];
        for (index, row) in rows.iter_mut().enumerate() {
            let scale = row[column];
            if index != column && (!reciprocal || scale != 0.0) {
                for (value, base) in row.iter_mut().zip(normalized) {
                    *value -= scale * base;
                }
            }
        }
    }
    let mut output = [0.0; 16];
    for (index, row) in rows.iter().enumerate() {
        output[index * 4..index * 4 + 4].copy_from_slice(&row[4..]);
    }
    Some(output)
}

/// Checks finite components against an absolute tolerance.
pub fn finite_matrix_close(a: &[f32; 16], b: &[f32; 16], tolerance: f32) -> bool {
    a.iter()
        .zip(b)
        .all(|(&a, &b)| a.is_finite() && b.is_finite() && (a - b).abs() <= tolerance)
}

#[cfg(test)]
mod inverse_tests {
    use super::*;

    #[test]
    fn inverse_preserves_reference_pivot_floors() {
        let mut matrix = [
            0.0, 2.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut expected = [
            0.0, 0.25, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.125, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        assert_eq!(inverse_4x4(&matrix), Some(expected));
        assert_eq!(
            inverse_4x4_with(&matrix, InversePolicy::Reciprocal),
            Some(expected)
        );
        matrix[15] = 2_f64.powi(-50);
        expected[15] = 2_f64.powi(50);
        assert_eq!(inverse_4x4(&matrix), None);
        assert_eq!(
            inverse_4x4_with(&matrix, InversePolicy::Reciprocal),
            Some(expected)
        );
        matrix[15] = 1e-301;
        assert_eq!(inverse_4x4_with(&matrix, InversePolicy::Reciprocal), None);
        matrix[15] = f64::INFINITY;
        assert_eq!(inverse_4x4_with(&matrix, InversePolicy::Reciprocal), None);
    }
}
