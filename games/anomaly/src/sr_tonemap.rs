// SPDX-License-Identifier: MIT
//! CPU oracle for the low tonemap and the stock UNORM8 conversion in the D3D11 fixtures.
//! See docs/research/anomaly-sr-seam.md section 2.2.

#[cfg(all(windows, feature = "sr-fixtures"))]
pub(crate) mod roundtrip_fixture;

#[cfg(feature = "sr-fixtures")]
const WHITE_SQUARED: f64 = 1.7 * 1.7;

#[cfg(feature = "sr-fixtures")]
pub(crate) extern "C" fn rsf_anomaly_tonemap_forward(linear: f64) -> f64 {
    linear * (1.0 + linear / WHITE_SQUARED) / (1.0 + linear)
}

#[cfg(feature = "sr-fixtures")]
pub(crate) extern "C" fn rsf_anomaly_tonemap_inverse(mapped: f64) -> f64 {
    let b = 1.0 - mapped;
    let d = (b * b + 4.0 * mapped / WHITE_SQUARED).sqrt();
    // Choose the form that avoids cancellation on each side of mapped = 1.
    if b > 0.0 {
        2.0 * mapped / (b + d)
    } else {
        0.5 * WHITE_SQUARED * (d - b)
    }
}

#[cfg(feature = "sr-fixtures")]
pub(crate) extern "C" fn rsf_anomaly_tonemap_unorm8(mapped: f64) -> i32 {
    if mapped.is_nan() || mapped <= 0.0 {
        return 0;
    }
    if mapped >= 1.0 {
        return 255;
    }
    (mapped * 255.0 + 0.5).floor() as i32
}
