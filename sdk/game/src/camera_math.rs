// SPDX-License-Identifier: MIT
//! Matrix operations shared by the native camera adapters.

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
