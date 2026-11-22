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
        if !rows[pivot][column].is_finite() || rows[pivot][column].abs() < 1e-12 {
            return None;
        }
        rows.swap(column, pivot);
        let divisor = rows[column][column];
        for value in &mut rows[column] {
            *value /= divisor;
        }
        let normalized = rows[column];
        for (index, row) in rows.iter_mut().enumerate() {
            if index != column {
                let scale = row[column];
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
