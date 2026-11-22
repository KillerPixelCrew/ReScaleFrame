// SPDX-License-Identifier: MIT
use super::*;

const BYTES: usize = RSF_AC7_VIEW_BUFFER_BYTES as usize;
const VIEW_STRUCT: u32 = size_of::<rsf_ac7_view>() as u32;
type Buffer = [u8; BYTES];

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

fn projection_buffer([width, height, x, y]: [f64; 4], [jx, jy]: [f32; 2]) -> Buffer {
    let (sin, cos) = 0.4_f64.sin_cos();
    let camera = [
        cos, 0.0, sin, 0.0, 0.0, 1.0, 0.0, 0.0, -sin, 0.0, cos, 0.0, 120.0, -40.0, 900.0, 1.0,
    ];
    let projection = [
        1.2,
        0.0,
        0.0,
        0.0,
        0.0,
        2.1,
        0.0,
        0.0,
        f64::from(jx),
        f64::from(jy),
        0.0,
        1.0,
        0.0,
        0.0,
        10.0,
        0.0,
    ];
    let mut translation = identity();
    translation[12..15].copy_from_slice(&[-5000.0, 250.0, 1200.0]);
    let to_clip = multiply(camera, projection);
    let world_to_clip = multiply(translation, to_clip);
    let from_clip = inverse_4x4(&to_clip).unwrap();
    let mut screen = projection;
    screen[0] = 1.0;
    screen[5] = 1.0;
    screen[8] = 0.0;
    screen[9] = 0.0;
    let mut sv = identity();
    sv[0] = 2.0 / width;
    sv[5] = -2.0 / height;
    sv[12] = -1.0 - 2.0 * x / width;
    sv[13] = 1.0 + 2.0 * y / height;
    let mut buffer = [0; BYTES];
    for (offset, matrix) in [
        (0x000, to_clip),
        (0x040, world_to_clip),
        (0x080, camera),
        (0x180, projection),
        (0x1c0, inverse_4x4(&projection).unwrap()),
        (0x200, from_clip),
        (0x240, multiply(sv, from_clip)),
        (
            0x280,
            multiply(screen, inverse_4x4(&world_to_clip).unwrap()),
        ),
        (0x2c0, multiply(screen, from_clip)),
    ] {
        write_floats(&mut buffer, offset, &matrix.map(|value| value as f32));
    }
    write_floats(&mut buffer, 0x720, &[jx, jy]);
    buffer
}

#[test]
fn unjittered_layer_preserves_projection_and_pixel_world_positions() {
    let unjitter = |source: &Buffer, target: &mut Buffer| unsafe {
        rsf_ac7_view_remove_jitter(
            source.as_ptr().cast(),
            target.as_mut_ptr().cast(),
            BYTES as u32,
        )
    };
    for rect in [
        [1024.0, 576.0, 0.0, 0.0],
        [800.0, 450.0, 0.0, 0.0],
        [1600.0, 900.0, 0.0, 0.0],
        [1600.0, 904.0, 32.0, 16.0],
    ] {
        let jittered = projection_buffer(rect, [0.00031, -0.00052]);
        let stable = projection_buffer(rect, [0.0; 2]);
        let mut removed = [0; BYTES];
        assert_eq!(unjitter(&jittered, &mut removed), 1);
        for offset in [0x000, 0x040, 0x180, 0x1c0, 0x200, 0x240, 0x280, 0x2c0] {
            for (actual, expected) in floats::<16>(&removed, offset)
                .into_iter()
                .zip(floats::<16>(&stable, offset))
            {
                near(actual, expected, 1e-4 * expected.abs().max(1.0));
            }
        }
        assert_eq!(floats::<2>(&removed, 0x720), [0.0; 2]);
        for depth in [0.01, 0.2, 0.8] {
            let pixel = [
                rect[2] + rect[0] * 0.37,
                rect[3] + rect[1] * 0.61,
                depth,
                1.0,
            ];
            let unproject = |buffer: &[u8]| {
                let matrix = floats::<16>(buffer, 0x240).map(f64::from);
                let point: [f64; 4] = std::array::from_fn(|column| {
                    (0..4)
                        .map(|row| pixel[row] * matrix[row * 4 + column])
                        .sum()
                });
                point.map(|value| value / point[3])
            };
            for (actual, expected) in unproject(&removed).into_iter().zip(unproject(&stable)) {
                assert!((actual - expected).abs() <= 1e-5 * expected.abs().max(1.0));
            }
        }
        assert_eq!(unjitter(&stable, &mut removed), 0);
        assert!(removed == stable);
    }
}
