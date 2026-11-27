// SPDX-License-Identifier: MIT
//! The classification of the `ReScaleFrameEffect` motion readback (`gdextension/effect.rs`).

/// One readback texel: the shader's `vec2` in a std430 array, two little-endian `f32`.
const SAMPLE_BYTES: usize = 8;
/// A finite texel moves when either component's magnitude exceeds this, in UV.
const MOVING_UV: f64 = 0.00001;

/// The last requested capture. An attempt that read nothing is still counted, unsucceeded.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Capture {
    pub(crate) count: u64,
    /// The callback count the capture was taken in.
    pub(crate) callback: u64,
    pub(crate) pixels: u64,
    pub(crate) moving_pixels: u64,
    pub(crate) nonfinite_pixels: u64,
    pub(crate) unwritten_pixels: u64,
    pub(crate) max_abs_uv: f64,
    pub(crate) succeeded: bool,
}

/// Classifies a readback of `pixels` texels of R16G16_SFLOAT motion in UV, decoded as Godot's
/// `PackedByteArray.decode_float` does (little-endian `f32` widened to `f64`). A readback of any
/// other length fails and counts nothing. Each texel is non-finite when either component is;
/// otherwise unwritten when both are at most -1 (Forward+ clears the velocity-only framebuffer to
/// (-1, -1) before its motion pass in the pinned 4.7.2 source); otherwise its larger magnitude
/// feeds the maximum and it moves above `MOVING_UV`. Valid zero motion is neither.
pub(crate) fn classify(bytes: &[u8], pixels: u64) -> Capture {
    let mut capture = Capture {
        pixels,
        succeeded: bytes.len() as u64 == pixels.wrapping_mul(SAMPLE_BYTES as u64),
        ..Capture::default()
    };
    if !capture.succeeded {
        return capture;
    }
    for &[x0, x1, x2, x3, y0, y1, y2, y3] in bytes.as_chunks::<SAMPLE_BYTES>().0 {
        let x = f64::from(f32::from_le_bytes([x0, x1, x2, x3]));
        let y = f64::from(f32::from_le_bytes([y0, y1, y2, y3]));
        if !x.is_finite() || !y.is_finite() {
            capture.nonfinite_pixels += 1;
        } else if x <= -1.0 && y <= -1.0 {
            capture.unwritten_pixels += 1;
        } else {
            let magnitude = x.abs().max(y.abs());
            if magnitude > MOVING_UV {
                capture.moving_pixels += 1;
            }
            capture.max_abs_uv = capture.max_abs_uv.max(magnitude);
        }
    }
    capture
}

#[cfg(test)]
mod tests;
