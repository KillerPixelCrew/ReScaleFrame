// SPDX-License-Identifier: MIT
use super::*;

fn valid_extent(extent: Extent) -> bool {
    (2..=16384).contains(&extent.width) && (2..=16384).contains(&extent.height)
}

pub(crate) fn resolutions(sizes: &Sizes) -> Option<Sizes> {
    ([sizes.output, sizes.render, sizes.pda]
        .into_iter()
        .all(valid_extent)
        && sizes.render.width <= sizes.output.width
        && sizes.render.height <= sizes.output.height)
        .then_some(*sizes)
}

fn finite(matrix: &Matrix) -> bool {
    matrix.values.iter().all(|value| value.is_finite())
}

pub(crate) fn affine(matrix: &Matrix) -> bool {
    finite(matrix)
        && matrix.values[12..15]
            .iter()
            .all(|value| value.abs() <= 1e-9)
        && (matrix.values[15] - 1.).abs() <= 1e-9
}

pub(crate) fn index(row: usize, column: usize) -> Option<usize> {
    if row < 4 && column < 4 {
        Some(row * 4 + column)
    } else {
        None
    }
}

pub(crate) fn read(matrix: &Matrix, row: usize, column: usize) -> Option<f64> {
    index(row, column).map(|index| matrix.values[index])
}

pub(crate) fn write(matrix: &mut Matrix, row: usize, column: usize, value: f64) -> bool {
    let Some(index) = index(row, column) else {
        return false;
    };
    matrix.values[index] = value;
    true
}

pub(crate) fn multiply(left: &Matrix, right: &Matrix) -> Option<Matrix> {
    if !finite(left) || !finite(right) {
        return None;
    }
    let value = Matrix {
        values: std::array::from_fn(|i| {
            let (row, column) = (i / 4, i % 4);
            (0..4)
                .map(|j| left.values[row * 4 + j] * right.values[j * 4 + column])
                .sum()
        }),
    };
    finite(&value).then_some(value)
}

// Keep the reference model's scaled pivot selection and its first-row tie break.
pub(crate) fn inverse(matrix: &Matrix) -> Option<Matrix> {
    if !finite(matrix) {
        return None;
    }
    let mut augmented = [[0.; 8]; 4];
    let mut scale = [0.; 4];
    for (i, row) in augmented.iter_mut().enumerate() {
        row[..4].copy_from_slice(&matrix.values[i * 4..i * 4 + 4]);
        scale[i] = row[..4].iter().map(|value| value.abs()).fold(0., f64::max);
        if scale[i] == 0. {
            return None;
        }
        row[i + 4] = 1.;
    }
    for column in 0..4 {
        let mut pivot = column;
        for row in column + 1..4 {
            if augmented[row][column].abs() / scale[row]
                > augmented[pivot][column].abs() / scale[pivot]
            {
                pivot = row;
            }
        }
        if augmented[pivot][column].abs() / scale[pivot] <= 1e-12 {
            return None;
        }
        augmented.swap(column, pivot);
        scale.swap(column, pivot);
        let divisor = augmented[column][column];
        for value in &mut augmented[column] {
            *value /= divisor;
        }
        let pivot_row = augmented[column];
        for (i, row) in augmented.iter_mut().enumerate() {
            if i != column {
                let factor = row[column];
                for (value, pivot) in row.iter_mut().zip(pivot_row) {
                    *value -= factor * pivot;
                }
            }
        }
    }
    let output = Matrix {
        values: std::array::from_fn(|i| augmented[i / 4][4 + i % 4]),
    };
    finite(&output).then_some(output)
}

pub(crate) fn finite_point(point: Vec4) -> bool {
    [point.x, point.y, point.z, point.w]
        .into_iter()
        .all(f64::is_finite)
}

pub(crate) fn transform(matrix: &Matrix, point: [f64; 4]) -> Option<[f64; 4]> {
    if !finite(matrix) || !point.into_iter().all(f64::is_finite) {
        return None;
    }
    let output: [f64; 4] = std::array::from_fn(|row| {
        (0..4)
            .map(|column| matrix.values[row * 4 + column] * point[column])
            .sum()
    });
    output.into_iter().all(f64::is_finite).then_some(output)
}

pub(crate) fn jitter(projection: &Matrix, x: f64, y: f64, render: Extent) -> Option<Matrix> {
    if !finite(projection) || !x.is_finite() || !y.is_finite() || !valid_extent(render) {
        return None;
    }
    let mut result = *projection;
    for column in 0..4 {
        let w = projection.values[12 + column];
        result.values[column] += (2. * x / f64::from(render.width)) * w;
        result.values[4 + column] -= (2. * y / f64::from(render.height)) * w;
    }
    finite(&result).then_some(result)
}

fn clip_uv(clip: [f64; 4]) -> Option<[f64; 2]> {
    let [x, y, z, w] = clip;
    if !clip.into_iter().all(f64::is_finite) || w <= 1e-9 || z < 0. || z > w {
        return None;
    }
    let uv = [0.5 * (x / w + 1.), 0.5 * (1. - y / w)];
    uv.into_iter().all(f64::is_finite).then_some(uv)
}

pub(crate) fn motion(
    current: [f64; 4],
    previous: [f64; 4],
    render: Extent,
    jitter: [f64; 2],
) -> Option<[f64; 2]> {
    let current = clip_uv(current)?;
    let previous = clip_uv(previous)?;
    if !valid_extent(render) || !jitter.into_iter().all(f64::is_finite) {
        return None;
    }
    let grid = [f64::from(render.width), f64::from(render.height)];
    let pixels: [f64; 2] = std::array::from_fn(|i| (previous[i] - current[i]) * grid[i]);
    if !pixels.into_iter().all(f64::is_finite) {
        return None;
    }
    let result = std::array::from_fn(|i| pixels[i] + jitter[i]);
    result.into_iter().all(f64::is_finite).then_some(result)
}
