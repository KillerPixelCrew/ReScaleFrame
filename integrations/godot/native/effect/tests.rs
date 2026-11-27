// SPDX-License-Identifier: MIT
use super::*;

fn samples(values: &[(f32, f32)]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|&(x, y)| x.to_le_bytes().into_iter().chain(y.to_le_bytes()))
        .collect()
}

const SIX: [(f32, f32); 6] = [
    (0.0, 0.0),
    (-1.0, -1.0),
    (f32::NAN, 0.0),
    (0.25, -0.5),
    (0.00001, 0.0),
    (-1.0, -0.5),
];

#[test]
fn classification_counts_each_texel_once() {
    let capture = classify(&samples(&SIX), 6);
    assert!(capture.succeeded);
    assert_eq!(capture.pixels, 6);
    assert_eq!(capture.nonfinite_pixels, 1);
    assert_eq!(capture.unwritten_pixels, 1);
    // 0.25/-0.5 and -1/-0.5 move. The f32 nearest 0.00001 widens below the f64 threshold, so it
    // is still, as Godot's decode_float compared in double.
    assert_eq!(capture.moving_pixels, 2);
    assert_eq!(capture.max_abs_uv, 1.0);
}

#[test]
fn classification_keeps_valid_zero_and_infinity_apart() {
    let capture = classify(
        &samples(&[(0.0, -0.0), (f32::INFINITY, 0.0), (-2.0, -1.0), (-1.0, 0.0)]),
        4,
    );
    assert_eq!(capture.nonfinite_pixels, 1);
    assert_eq!(capture.unwritten_pixels, 1);
    assert_eq!(capture.moving_pixels, 1);
    assert_eq!(capture.max_abs_uv, 1.0);
}

#[test]
fn a_readback_of_the_wrong_length_fails_and_counts_nothing() {
    for bytes in [Vec::new(), samples(&SIX[..5]), samples(&SIX)[..47].to_vec()] {
        let capture = classify(&bytes, 6);
        assert_eq!(
            capture,
            Capture {
                pixels: 6,
                ..Capture::default()
            }
        );
    }
}
