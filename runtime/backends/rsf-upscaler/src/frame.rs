//! Per-frame values a backend needs, converted out of the units the engine keeps them in.
//!
//! A frame capture cannot supply any of this. Jitter, camera matrices and the motion scale live in
//! the view uniform data, which is why the loader reads that buffer live. What arrives from there
//! is in Unreal's units: jitter in clip space, motion in a screen space that spans two units across
//! the viewport. Every backend wants pixels. The conversions are small and each one has a factor
//! that is wrong by two or four if it is guessed at, so they live here with the reasoning attached
//! rather than being written out again at each call site.

use crate::Extent;

/// A sub-pixel projection offset, in pixels at render resolution.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jitter {
    pub x: f32,
    pub y: f32,
}

impl Jitter {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Recover pixels from Unreal's `TemporalAAJitter`, which is stored in clip space.
    ///
    /// The engine builds that field as `JitterPixels.x * 2 / ViewRect.Width` with the vertical term
    /// negated, so this is that construction run backwards. Two things about it:
    ///
    /// The divisor is the view rect, not the buffer. At reduced screen percentage those differ, and
    /// using the buffer size would scale every offset by the render scale without ever looking
    /// wrong, so this takes the render extent and callers must pass the render rect.
    ///
    /// The horizontal term is confirmed: offsets of -0.147, 0.065 and -0.430 pixels came back
    /// through it from the running game, all inside the plus or minus half pixel the sample pattern
    /// produces. The vertical sign flip follows the engine's construction and has not been checked
    /// against a rendered result. A wrong sign there survives a still camera and smears under
    /// vertical motion, so it is the first thing to test once a backend is running.
    #[must_use]
    pub fn from_unreal_clip(clip: [f32; 2], render: Extent) -> Self {
        Self {
            x: clip[0] * (render.width as f32) * 0.5,
            y: clip[1] * (render.height as f32) * -0.5,
        }
    }

    /// Whether the offset is inside the half pixel a sample pattern can produce.
    ///
    /// Anything outside it means the conversion is wrong rather than the game being unusual, which
    /// is worth catching at the point the value is read rather than in a smeared image.
    #[must_use]
    pub fn within_half_pixel(self) -> bool {
        self.x.abs() <= 0.5 && self.y.abs() <= 0.5
    }
}

/// How many offsets the jitter sequence should cycle through before repeating.
///
/// Backends want the sequence to grow with the area ratio, so eight at native and thirty two at
/// half scale: each output pixel has to be covered by roughly the same number of distinct samples
/// however many render pixels there are behind it.
///
/// Unreal 4.18 will not do this by itself. It takes the count from `r.TemporalAASamples` and that
/// value does not move with screen percentage, so setting the render scale without also setting the
/// count leaves the reconstruction with a quarter of the samples it expects. Since the loader
/// already writes console variables, this is what it should write.
#[must_use]
pub fn recommended_phase_count(render: Extent, output: Extent) -> u32 {
    if render.width == 0 {
        return 8;
    }
    let ratio = (output.width as f32) / (render.width as f32);
    // Rounded, not rounded up. Render sizes are whole pixels, so a ratio meant to be exactly 1.5
    // arrives as 1.50055 and rounding up would spend an extra phase on that.
    let count = (8.0 * ratio * ratio).round() as u32;
    count.max(8)
}

/// Turns decoded motion into pixels at render resolution.
///
/// Decoded Unreal velocity is a difference between two screen positions in clip coordinates, which
/// span two units across the viewport rather than one. So the factor is half the extent, not the
/// extent, and the vertical axis is negated because clip space runs up while pixels run down.
///
/// This is the second factor of two in the same value. The first is in the storage encoding, in
/// [`crate::Encoding::UNREAL_G16R16`]. Getting one right and the other wrong gives motion that
/// points the right way and is twice as long as it should be, which reads as a slightly too eager
/// reconstruction rather than as a bug.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionToPixels {
    pub x: f32,
    pub y: f32,
}

impl MotionToPixels {
    #[must_use]
    pub fn unreal(render: Extent) -> Self {
        Self {
            x: (render.width as f32) * 0.5,
            y: (render.height as f32) * -0.5,
        }
    }

    /// Apply the scale to one decoded motion vector.
    #[must_use]
    pub fn apply(self, decoded: [f32; 2]) -> [f32; 2] {
        [decoded[0] * self.x, decoded[1] * self.y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Quality;

    const RENDER: Extent = Extent::new(1024, 576);

    #[test]
    fn jitter_survives_the_round_trip_through_clip_space() {
        for pixels in [-0.5_f32, -0.147, 0.0, 0.065, 0.43] {
            let clip = [
                pixels * 2.0 / (RENDER.width as f32),
                pixels * -2.0 / (RENDER.height as f32),
            ];
            let recovered = Jitter::from_unreal_clip(clip, RENDER);
            assert!((recovered.x - pixels).abs() < 1e-5, "x {pixels}");
            assert!((recovered.y - pixels).abs() < 1e-5, "y {pixels}");
        }
    }

    #[test]
    fn the_measured_offset_comes_back_as_recorded() {
        // -0.147 pixels at 2048 wide, the value read from the running game after the jitter gate
        // was patched. A divisor of the buffer rather than the view rect, or a missing factor of
        // two, both land somewhere else.
        let output = Extent::new(2048, 1152);
        let clip = [-0.000_143_55_f32, 0.0];
        let jitter = Jitter::from_unreal_clip(clip, output);
        assert!((jitter.x - -0.147).abs() < 1e-3, "got {}", jitter.x);
        assert!(jitter.within_half_pixel());
    }

    #[test]
    fn an_offset_outside_half_a_pixel_is_not_accepted_as_jitter() {
        assert!(!Jitter::new(0.9, 0.0).within_half_pixel());
        assert!(Jitter::new(-0.5, 0.5).within_half_pixel());
    }

    #[test]
    fn the_phase_count_follows_the_area_ratio() {
        let output = Extent::new(2048, 1152);
        assert_eq!(recommended_phase_count(output, output), 8);
        assert_eq!(recommended_phase_count(Extent::new(1024, 576), output), 32);
        // The quality ratio. 2048/1.5 is not a whole number of pixels, so the ratio arrives as
        // 1.50055 rather than 1.5 and the count must still come out at 8 * 1.5^2.
        assert_eq!(
            recommended_phase_count(Quality::Quality.render_size(output), output),
            18
        );
    }

    #[test]
    fn the_phase_count_never_drops_below_the_engine_default() {
        let output = Extent::new(2048, 1152);
        assert_eq!(recommended_phase_count(Extent::new(4096, 2304), output), 8);
        assert_eq!(recommended_phase_count(Extent::new(0, 0), output), 8);
    }

    #[test]
    fn motion_of_one_clip_unit_is_half_the_viewport() {
        let scale = MotionToPixels::unreal(RENDER);
        let pixels = scale.apply([1.0, 1.0]);
        assert!((pixels[0] - 512.0).abs() < 1e-3);
        // Negated, because clip space runs up and pixels run down.
        assert!((pixels[1] - -288.0).abs() < 1e-3);
    }

    #[test]
    fn the_largest_motion_measured_in_the_game_is_a_few_pixels() {
        // Raw 32813 in a frame with ground in view decodes to 0.0028 in screen space. At 2048 wide
        // that is roughly 2.9 pixels, which is what the capture research recorded. A factor of two
        // wrong anywhere in the chain shows up here.
        let decoded = crate::Encoding::UNREAL_G16R16.decode(32813.0 / 65535.0);
        let pixels = MotionToPixels::unreal(Extent::new(2048, 1152)).apply([decoded, 0.0]);
        assert!((pixels[0] - 2.9).abs() < 0.2, "got {}", pixels[0]);
    }
}
