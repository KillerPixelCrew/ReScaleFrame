//! Convert engine jitter and motion units into the pixel units expected by reconstruction backends.

use crate::Extent;

/// A sub-pixel projection offset, in pixels at render resolution.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Jitter {
    pub x: f32,
    pub y: f32,
}

impl Jitter {
    /// Construct a pixel offset without finiteness or range validation.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Recover pixels from Unreal's `TemporalAAJitter`, which is stored in clip space.
    ///
    /// Inverts UE4's clip-space conversion. `render` must be the view rectangle extent, not the
    /// backing texture size. The vertical component is negated to match the pixel-space Y axis.
    #[must_use]
    pub fn from_unreal_clip(clip: [f32; 2], render: Extent) -> Self {
        Self {
            x: clip[0] * (render.width as f32) * 0.5,
            y: clip[1] * (render.height as f32) * -0.5,
        }
    }

    /// Whether both components fall within the temporal sample range of ±0.5 pixels.
    #[must_use]
    pub fn within_half_pixel(self) -> bool {
        self.x.abs() <= 0.5 && self.y.abs() <= 0.5
    }
}

/// Recommend a jitter phase count proportional to the square of the horizontal output-to-render
/// scale, with an eight-phase minimum. The caller is expected to preserve aspect ratio.
/// A zero render width returns the base count; height does not participate in this approximation.
#[must_use]
pub fn recommended_phase_count(render: Extent, output: Extent) -> u32 {
    if render.width == 0 {
        return 8;
    }
    let ratio = (output.width as f32) / (render.width as f32);
    // Round to the nearest phase to avoid over-counting ratios affected by integer render sizes.
    let count = (8.0 * ratio * ratio).round() as u32;
    count.max(8)
}

/// Scale decoded clip-space motion into render-resolution pixels. Each axis uses half the render
/// extent; Y is negated because clip space and pixel space have opposite vertical directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionToPixels {
    pub x: f32,
    pub y: f32,
}

impl MotionToPixels {
    /// Construct UE clip-to-pixel multipliers for the active view rectangle.
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
        // Recorded -0.147-pixel offset at 2048 view width checks the view extent and half factor.
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
        // Integer rounding makes 2048/1365 slightly exceed 1.5; the phase count still rounds to 18.
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
        // Recorded raw 32813 decodes to about 0.0028 clip units, or 2.9 pixels at 2048 view width.
        let decoded = crate::Encoding::UNREAL_G16R16.decode(32813.0 / 65535.0);
        let pixels = MotionToPixels::unreal(Extent::new(2048, 1152)).apply([decoded, 0.0]);
        assert!((pixels[0] - 2.9).abs() < 0.2, "got {}", pixels[0]);
    }
}
