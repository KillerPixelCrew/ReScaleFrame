// SPDX-License-Identifier: MIT
//! AC7's verified UE4.18 view buffer layout, camera decode and UI projection jitter removal.
//! See docs/research/ac7-frame-capture.md.
use crate::view_abi::{
    RSF_AC7_VIEW_ABI_VERSION, RSF_AC7_VIEW_BUFFER_BYTES, rsf_ac7_view, rsf_ac7_view_result,
};
use rsf_game_sys::camera_math::inverse_4x4;
use std::ffi::c_void;
use std::{ptr, slice};

const OK: rsf_ac7_view_result = 0;
const INVALID_ARGUMENT: rsf_ac7_view_result = -1;
const ABI_MISMATCH: rsf_ac7_view_result = -2;
const NOT_A_VIEW: rsf_ac7_view_result = -3;
const NOT_PERSPECTIVE: rsf_ac7_view_result = -4;

const TRANSLATED_WORLD_TO_CLIP: usize = 0x000;
const WORLD_TO_CLIP: usize = 0x040;
const VIEW_TO_TRANSLATED_WORLD: usize = 0x0c0;
const VIEW_TO_CLIP: usize = 0x180;
const CLIP_TO_VIEW: usize = 0x1c0;
const CLIP_TO_TRANSLATED_WORLD: usize = 0x200;
const SV_POSITION_TO_TRANSLATED_WORLD: usize = 0x240;
const SCREEN_TO_WORLD: usize = 0x280;
const SCREEN_TO_TRANSLATED_WORLD: usize = 0x2c0;
const VIEW_FORWARD: usize = 0x300;
const VIEW_UP: usize = 0x310;
const VIEW_RIGHT: usize = 0x320;
const WORLD_CAMERA_ORIGIN: usize = 0x370;
const PRE_VIEW_TRANSLATION: usize = 0x3a0;
const CLIP_TO_PREV_CLIP: usize = 0x6e0;
const JITTER: usize = 0x720;
const VIEW_RECT_MIN: usize = 0x7e0;
const VIEW_SIZE: usize = 0x7f0;
const BUFFER_SIZE: usize = 0x800;

type Matrix = [f32; 16];

fn floats<const N: usize>(data: &[u8], offset: usize) -> [f32; N] {
    std::array::from_fn(|index| {
        let offset = offset + index * 4;
        f32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ])
    })
}

fn write_floats(data: &mut [u8], offset: usize, floats: &[f32]) {
    for (bytes, value) in data[offset..offset + floats.len() * 4]
        .chunks_exact_mut(4)
        .zip(floats)
    {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
}

fn finite(value: f32) -> bool {
    value.abs() < 1e30
}

fn size_and_inverse([width, height, inv_width, inv_height]: [f32; 4]) -> bool {
    [(width, inv_width), (height, inv_height)]
        .iter()
        .all(|&(size, inverse)| {
            (1.0..=65_536.0).contains(&size)
                && finite(inverse)
                && (size * inverse - 1.0).abs() <= 1e-3
        })
}

fn basis_matches(matrix: &Matrix, basis: [[f32; 3]; 3]) -> bool {
    for (row, [x, y, z]) in basis.into_iter().enumerate() {
        let length = (x * x + y * y + z * z).sqrt();
        if !finite(length) || (length - 1.0).abs() > 1e-3 {
            return false;
        }
        if ![x, y, z]
            .iter()
            .zip(&matrix[row * 4..row * 4 + 3])
            .all(|(&component, &expected)| (component - expected).abs() <= 1e-3)
        {
            return false;
        }
    }
    true
}

fn projection_matches(view_to_clip: &Matrix, clip_to_view: &Matrix) -> bool {
    for row in 0..4 {
        for column in 0..4 {
            let mut product = 0.0;
            for k in 0..4 {
                product +=
                    f64::from(view_to_clip[row * 4 + k]) * f64::from(clip_to_view[k * 4 + column]);
            }
            let expected = if row == column { 1.0 } else { 0.0 };
            if !product.is_finite() || (product - expected).abs() > 0.01 {
                return false;
            }
        }
    }
    true
}

fn inverse(matrix: Matrix) -> Option<Matrix> {
    let inverse = inverse_4x4(&matrix.map(f64::from))?.map(|value| value as f32);
    inverse
        .iter()
        .all(|value| value.is_finite())
        .then_some(inverse)
}

fn read_view(data: &[u8], struct_size: u32) -> Result<rsf_ac7_view, rsf_ac7_view_result> {
    let view_size = floats(data, VIEW_SIZE);
    let buffer_size = floats(data, BUFFER_SIZE);
    if !size_and_inverse(view_size) || !size_and_inverse(buffer_size) {
        return Err(NOT_A_VIEW);
    }
    let right = floats(data, VIEW_RIGHT);
    let up = floats(data, VIEW_UP);
    let forward = floats(data, VIEW_FORWARD);
    let basis = floats(data, VIEW_TO_TRANSLATED_WORLD);
    if !basis_matches(&basis, [right, up, forward]) {
        return Err(NOT_A_VIEW);
    }
    let origin = floats::<3>(data, WORLD_CAMERA_ORIGIN);
    let translation = floats::<3>(data, PRE_VIEW_TRANSLATION);
    if !origin
        .iter()
        .zip(translation)
        .all(|(&position, translation)| {
            finite(position) && finite(translation) && (position + translation).abs() <= 1.0
        })
    {
        return Err(NOT_A_VIEW);
    }

    let view_to_clip = floats(data, VIEW_TO_CLIP);
    let clip_to_view = floats(data, CLIP_TO_VIEW);
    if !projection_matches(&view_to_clip, &clip_to_view) {
        return Err(NOT_A_VIEW);
    }
    let clip_to_prev_clip = floats(data, CLIP_TO_PREV_CLIP);
    let prev_clip_to_clip = inverse(clip_to_prev_clip).ok_or(NOT_A_VIEW)?;
    let horizontal_scale = view_to_clip[0];
    let vertical_scale = view_to_clip[5];
    if (view_to_clip[11] - 1.0).abs() > 1e-3 || horizontal_scale <= 0.0 || vertical_scale <= 0.0 {
        return Err(NOT_PERSPECTIVE);
    }
    let aspect_ratio = vertical_scale / horizontal_scale;
    let [rect_x, rect_y] = floats(data, VIEW_RECT_MIN);
    let rect_valid = |value: f32| (0.0..=f64::from(u32::MAX)).contains(&f64::from(value + 0.5));
    let [jx, jy, prev_x, prev_y] = floats(data, JITTER);
    if !aspect_ratio.is_finite()
        || !rect_valid(rect_x)
        || !rect_valid(rect_y)
        || ![jx, jy, prev_x, prev_y].into_iter().all(finite)
    {
        return Err(NOT_A_VIEW);
    }
    let round = |value: f32| (value + 0.5) as u32;
    let view_width = round(view_size[0]);
    let view_height = round(view_size[1]);
    let buffer_width = round(buffer_size[0]);
    let buffer_height = round(buffer_size[1]);
    let view_rect_x = round(rect_x);
    let view_rect_y = round(rect_y);
    let half_width = view_width as f32 * 0.5;
    let half_height = view_height as f32 * 0.5;
    let mut view_to_clip_no_jitter = view_to_clip;
    view_to_clip_no_jitter[8] -= jx;
    view_to_clip_no_jitter[9] -= jy;
    let clip_to_view_no_jitter = inverse(view_to_clip_no_jitter).ok_or(NOT_A_VIEW)?;
    let is_main_view = view_width == buffer_width
        && view_height == buffer_height
        && view_rect_x == 0
        && view_rect_y == 0;
    Ok(rsf_ac7_view {
        struct_size,
        view_to_clip,
        view_to_clip_no_jitter,
        clip_to_view,
        clip_to_view_no_jitter,
        clip_to_prev_clip,
        prev_clip_to_clip,
        camera_position: origin,
        camera_forward: forward,
        camera_up: up,
        camera_right: right,
        near_plane: view_to_clip[14],
        vertical_fov: 2.0 * (1.0 / vertical_scale).atan(),
        aspect_ratio,
        view_width,
        view_height,
        view_rect_x,
        view_rect_y,
        buffer_width,
        buffer_height,
        is_main_view: u32::from(is_main_view),
        jitter_pixels: [jx * half_width, jy * -half_height],
        previous_jitter_pixels: [prev_x * half_width, prev_y * -half_height],
        has_jitter: u32::from(jx != 0.0 || jy != 0.0),
    })
}

/// The caller lends `buffer` for `bytes` and a size-prefixed writable output, or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_view_read(
    buffer: *const c_void,
    bytes: u32,
    abi_version: u32,
    out: *mut rsf_ac7_view,
) -> rsf_ac7_view_result {
    if buffer.is_null() || out.is_null() {
        return INVALID_ARGUMENT;
    }
    let size = unsafe { out.cast::<u32>().read() };
    if (size as usize) < size_of::<rsf_ac7_view>() {
        return INVALID_ARGUMENT;
    }
    if abi_version != RSF_AC7_VIEW_ABI_VERSION {
        return ABI_MISMATCH;
    }
    if bytes != RSF_AC7_VIEW_BUFFER_BYTES {
        return NOT_A_VIEW;
    }
    let data = unsafe { slice::from_raw_parts(buffer.cast::<u8>(), bytes as usize) };
    match read_view(data, size) {
        Ok(view) => {
            unsafe { out.write(view) };
            OK
        }
        Err(result) => result,
    }
}

/// Add the clip-space correction to a row of an inverse/world-space matrix.
fn add_rows(matrix: &mut Matrix, row: usize, basis: &[f32; 8], jx: f32, jy: f32) {
    for column in 0..4 {
        matrix[row * 4 + column] += jx * basis[column] + jy * basis[4 + column];
    }
}

fn remove_jitter(data: &mut [u8]) -> bool {
    let [jx, jy] = floats(data, JITTER);
    if (jx == 0.0 && jy == 0.0) || !finite(jx) || !finite(jy) {
        return false;
    }
    let projection: Matrix = floats(data, VIEW_TO_CLIP);
    if [projection[3], projection[7], projection[11], projection[15]] != [0.0, 0.0, 1.0, 0.0] {
        return false;
    }
    // For an into-clip matrix, multiply by the inverse of the jitter translation.
    for offset in [TRANSLATED_WORLD_TO_CLIP, WORLD_TO_CLIP, VIEW_TO_CLIP] {
        let mut matrix: Matrix = floats(data, offset);
        for row in matrix.chunks_exact_mut(4) {
            row[0] -= jx * row[3];
            row[1] -= jy * row[3];
        }
        write_floats(data, offset, &matrix);
    }
    // The first two rows are unchanged by the inverse correction. SVPosition uses their
    // world-space values with device depth, while ScreenToWorld uses linear depth.
    let world_basis = floats(data, CLIP_TO_TRANSLATED_WORLD);
    for (offset, row) in [
        (CLIP_TO_VIEW, 3),
        (CLIP_TO_TRANSLATED_WORLD, 3),
        (SV_POSITION_TO_TRANSLATED_WORLD, 3),
        (SCREEN_TO_WORLD, 2),
        (SCREEN_TO_TRANSLATED_WORLD, 2),
    ] {
        let mut matrix = floats(data, offset);
        let basis = if offset == SV_POSITION_TO_TRANSLATED_WORLD {
            world_basis
        } else {
            floats(data, offset)
        };
        add_rows(&mut matrix, row, &basis, jx, jy);
        write_floats(data, offset, &matrix);
    }
    write_floats(data, JITTER, &[0.0, 0.0]);
    true
}

/// The caller lends readable and writable buffers of `bytes` each, or null.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_ac7_view_remove_jitter(
    input: *const c_void,
    output: *mut c_void,
    bytes: u32,
) -> u32 {
    let bytes = bytes as usize;
    if input.is_null() || output.is_null() || bytes < BUFFER_SIZE {
        return 0;
    }
    let output = output.cast::<u8>();
    unsafe { ptr::copy(input.cast::<u8>(), output, bytes) };
    let data = unsafe { slice::from_raw_parts_mut(output, bytes) };
    u32::from(remove_jitter(data))
}
