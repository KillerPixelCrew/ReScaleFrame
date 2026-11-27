// SPDX-License-Identifier: MIT
use super::*;
use std::ptr;

const BYTES: usize = RSF_AC7_VIEW_BUFFER_BYTES as usize;
const VIEW_STRUCT: u32 = size_of::<rsf_ac7_view>() as u32;
type Buffer = [u8; BYTES];

fn write_floats(data: &mut [u8], offset: usize, floats: &[f32]) {
    for (bytes, value) in data[offset..offset + floats.len() * 4]
        .chunks_exact_mut(4)
        .zip(floats)
    {
        bytes.copy_from_slice(&value.to_le_bytes());
    }
}

fn view_buffer(view: [f32; 2], target: [f32; 2]) -> Buffer {
    let mut buffer = [0; BYTES];
    let mut put = |offset, values: &[f32]| write_floats(&mut buffer, offset, values);
    put(VIEW_RIGHT, &[0.0, 1.0, 0.0]);
    put(VIEW_UP, &[0.0, 0.0, 1.0]);
    put(VIEW_FORWARD, &[1.0, 0.0, 0.0]);
    put(
        VIEW_TO_TRANSLATED_WORLD,
        &[
            0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
    );
    let (x, y) = (1.63185, 2.90107);
    put(
        VIEW_TO_CLIP,
        &[
            x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
        ],
    );
    put(
        CLIP_TO_VIEW,
        &[
            x.recip(),
            0.0,
            0.0,
            0.0,
            0.0,
            y.recip(),
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            1.0,
            0.0,
        ],
    );
    let position = [260_968.45, 107_399.9, 21_228.2];
    put(WORLD_CAMERA_ORIGIN, &position);
    put(PRE_VIEW_TRANSLATION, &position.map(|value| -value));
    put(
        CLIP_TO_PREV_CLIP,
        &[
            1.0, -0.00009, 0.0, 0.0, 0.00003, 1.0, 0.0, 0.0, -0.02808, -0.20304, 1.0, 17.49332,
            -0.00001, 0.00003, 0.0, 1.0,
        ],
    );
    for (offset, [w, h]) in [(VIEW_SIZE, view), (BUFFER_SIZE, target)] {
        put(offset, &[w, h, w.recip(), h.recip()]);
    }
    buffer
}

fn near(actual: f32, expected: f32, tolerance: f32) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} differs from {expected}"
    );
}

#[test]
fn perspective_camera_reports_geometry_and_jitter_in_view_pixels() {
    let extent = [2048.0, 1152.0];
    let mut buffer = view_buffer(extent, extent);
    let view = read_view(&buffer, VIEW_STRUCT).expect("perspective view");
    assert_eq!([view.view_width, view.view_height], [2048, 1152]);
    assert_eq!(view.fills_target, 1);
    assert_eq!(view.has_jitter, 0);
    near(view.aspect_ratio, 16.0 / 9.0, 0.001);
    near(view.vertical_fov, 2.0 * (1.0_f32 / 2.90107).atan(), 1e-5);
    near(view.near_plane, 1.0, 1e-5);
    near(view.camera_position[0], 260_968.45, 1.0);
    assert_eq!(view.camera_forward, [1.0, 0.0, 0.0]);
    assert_eq!(view.camera_up, [0.0, 0.0, 1.0]);
    assert_eq!(view.camera_right, [0.0, 1.0, 0.0]);
    near(view.clip_to_prev_clip[11], 17.49332, 1e-4);
    let product = multiply(
        view.clip_to_prev_clip.map(f64::from),
        view.prev_clip_to_clip.map(f64::from),
    );
    for (actual, expected) in product.into_iter().zip(identity()) {
        near(actual as f32, expected as f32, 1e-4);
    }

    let jitter = [-0.00042, 0.00027, 0.000168, 0.00025];
    write_floats(&mut buffer, JITTER, &jitter);
    write_floats(&mut buffer, VIEW_TO_CLIP + 32, &jitter[..2]);
    let view = read_view(&buffer, VIEW_STRUCT).expect("jittered view");
    assert_eq!(view.has_jitter, 1);
    near(view.jitter_pixels[0], jitter[0] * 1024.0, 1e-4);
    near(view.jitter_pixels[1], jitter[1] * -576.0, 1e-4);
    assert!(view.jitter_pixels.iter().all(|jitter| jitter.abs() <= 0.5));
    near(view.previous_jitter_pixels[0], jitter[2] * 1024.0, 1e-4);
    near(view.view_to_clip[8], jitter[0], 1e-6);
    near(view.view_to_clip_no_jitter[8], 0.0, 1e-6);
    near(view.view_to_clip_no_jitter[9], 0.0, 1e-6);
    near(view.view_to_clip_no_jitter[0], view.view_to_clip[0], 1e-6);

    let mut partial = view_buffer([1016.0, 1016.0], extent);
    assert_eq!(read_view(&partial, VIEW_STRUCT).unwrap().fills_target, 0);
    write_floats(&mut partial, VIEW_RECT_MIN, &[32.0, 16.0]);
    assert_eq!(read_view(&partial, VIEW_STRUCT).unwrap().fills_target, 0);
}

#[test]
fn decoder_refuses_invalid_records_without_overwriting_output() {
    let good = view_buffer([2048.0, 1152.0], [2048.0, 1152.0]);
    let mut out = read_view(&good, VIEW_STRUCT).unwrap();
    let read = |buffer: *const u8, bytes, abi, out: &mut rsf_ac7_view| unsafe {
        rsf_ac7_view_read(buffer.cast(), bytes, abi, out)
    };
    assert_eq!(
        read(
            good.as_ptr(),
            BYTES as u32,
            RSF_AC7_VIEW_ABI_VERSION + 1,
            &mut out
        ),
        ABI_MISMATCH
    );
    assert_eq!(
        read(
            ptr::null(),
            BYTES as u32,
            RSF_AC7_VIEW_ABI_VERSION,
            &mut out
        ),
        INVALID_ARGUMENT
    );
    assert_eq!(
        read(good.as_ptr(), 2640, RSF_AC7_VIEW_ABI_VERSION, &mut out),
        NOT_A_VIEW
    );

    let mut unaligned = [0; BYTES + 1];
    unaligned[1..].copy_from_slice(&good);
    assert_eq!(
        read(
            unaligned[1..].as_ptr(),
            BYTES as u32,
            RSF_AC7_VIEW_ABI_VERSION,
            &mut out
        ),
        OK
    );
    let invalid: [(usize, &[f32]); 4] = [
        (CLIP_TO_VIEW, &[f32::NAN]),
        (VIEW_UP, &[0.5, 0.5, 0.5, 0.0]),
        (PRE_VIEW_TRANSLATION, &[1.0, 2.0, 3.0, 0.0]),
        (BUFFER_SIZE, &[2048.0, 1152.0, 0.5, 0.5]),
    ];
    for (offset, values) in invalid {
        let mut broken = good;
        write_floats(&mut broken, offset, values);
        assert_eq!(
            read(
                broken.as_ptr(),
                BYTES as u32,
                RSF_AC7_VIEW_ABI_VERSION,
                &mut out
            ),
            NOT_A_VIEW
        );
        assert_eq!(out.view_width, 2048);
        assert_eq!(out.clip_to_view, floats(&good, CLIP_TO_VIEW));
    }
    let mut noise = [0; BYTES];
    for index in 0..BYTES / 4 {
        write_floats(
            &mut noise,
            index * 4,
            &[(index * 37 % 211) as f32 * 0.01 - 1.0],
        );
    }
    assert_eq!(read_view(&noise, VIEW_STRUCT).err(), Some(NOT_A_VIEW));

    let mut ortho = good;
    write_floats(
        &mut ortho,
        VIEW_TO_CLIP,
        &[
            1.0 / 1024.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0 / 576.0,
            0.0,
            0.0,
            0.0,
            0.0,
            -1.0 / 200.0,
            0.0,
            -1.0,
            1.0,
            0.5,
            1.0,
        ],
    );
    write_floats(
        &mut ortho,
        CLIP_TO_VIEW,
        &[
            1024.0, 0.0, 0.0, 0.0, 0.0, -576.0, 0.0, 0.0, 0.0, 0.0, -200.0, 0.0, 1024.0, 576.0,
            100.0, 1.0,
        ],
    );
    assert_eq!(read_view(&ortho, VIEW_STRUCT).err(), Some(NOT_PERSPECTIVE));
}

type Mat = [f64; 16];

fn identity() -> Mat {
    std::array::from_fn(|index| f64::from(u8::from(index % 5 == 0)))
}

fn multiply(a: Mat, b: Mat) -> Mat {
    std::array::from_fn(|index| {
        let (row, column) = (index / 4, index % 4);
        (0..4).map(|k| a[row * 4 + k] * b[k * 4 + column]).sum()
    })
}
