//! How a game's motion vectors are actually stored, and what a backend needs them to be.
//!
//! This exists because the two are rarely the same. Ace Combat 7 writes object motion only,
//! biased into a sixteen bit unsigned target, and reserves zero to mean "nothing wrote here".
//! Handing that to a backend unchanged gives every static pixel a zero vector while the camera
//! moves, which is the input that produces smeared reconstruction.

/// What a value of zero in the motion vector target means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearBehaviour {
    /// Zero is a real motion vector of zero.
    RealMotion,
    /// Zero means nothing wrote this pixel, so its motion has to come from elsewhere.
    ///
    /// Unreal uses this. `EncodeVelocityToTexture` scales by `0.499 * 0.5` rather than `0.5`
    /// precisely to keep the encoded range clear of zero so it can serve as this sentinel.
    Unwritten,
}

/// How stored values map onto screen space motion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Encoding {
    /// Multiply the stored value by this after subtracting the bias.
    pub scale: f32,
    /// Subtract this from the stored value before scaling.
    pub bias: f32,
}

impl Encoding {
    /// Values already in screen space, needing no conversion.
    pub const DIRECT: Self = Self {
        scale: 1.0,
        bias: 0.0,
    };

    /// Unreal's biased encoding for a sixteen bit unsigned target.
    ///
    /// From `Common.ush`: `In * (0.499 * 0.5) + 32767/65535`. Taken from engine source rather
    /// than fitted to captured data, because in a frame where nothing moves much the samples
    /// cannot determine the scale and a fitted value is wrong by four times while looking
    /// entirely reasonable.
    pub const UNREAL_G16R16: Self = Self {
        scale: 1.0 / (0.499 * 0.5),
        bias: 32767.0 / 65535.0,
    };

    /// Decode one stored component.
    #[must_use]
    pub fn decode(self, stored: f32) -> f32 {
        (stored - self.bias) * self.scale
    }
}

/// Everything a backend has to be told about a game's motion vectors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionVectors {
    /// Whether camera movement is already folded into the stored vectors.
    ///
    /// When it is not, a backend that can reconstruct camera motion from depth and a
    /// current-to-previous transform should be told to do so, rather than the integration
    /// building a combined field itself. Streamline does this given `cameraMotionIncluded` and
    /// an invalid value; XeSS and FSR want a complete field and need the composition done first.
    pub camera_motion_included: bool,
    /// What an untouched pixel looks like.
    pub clear: ClearBehaviour,
    /// How stored values become screen space motion.
    pub encoding: Encoding,
}

impl MotionVectors {
    /// The convention Unreal 4.18 writes, and Ace Combat 7 with it.
    #[must_use]
    pub fn unreal_object_only() -> Self {
        Self {
            camera_motion_included: false,
            clear: ClearBehaviour::Unwritten,
            encoding: Encoding::UNREAL_G16R16,
        }
    }

    /// The same convention once a pass has decoded it into plain screen space values.
    ///
    /// Camera motion is untouched by that pass, so it stays absent, and the sentinel has to change:
    /// a decoded zero is a real zero motion, where a stored zero could not be.
    #[must_use]
    pub fn decoded(self) -> Self {
        Self {
            encoding: Encoding::DIRECT,
            clear: ClearBehaviour::RealMotion,
            ..self
        }
    }

    /// Whether the integration has to build a combined field before handing these over.
    ///
    /// A backend that cannot reconstruct camera motion needs one that already contains it.
    #[must_use]
    pub fn needs_composition_for(self, backend_reconstructs_camera_motion: bool) -> bool {
        !self.camera_motion_included && !backend_reconstructs_camera_motion
    }

    /// Whether a pass has to decode these before any backend can read them.
    ///
    /// Backends take a scale factor and nothing else, so an encoding that is a pure scale can be
    /// folded into that factor and one carrying a bias cannot. Unreal's carries a bias, and a
    /// backend handed the raw target reads a large constant motion across a still image.
    #[must_use]
    pub fn needs_decode(self) -> bool {
        self.encoding.bias != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreal_encoding_round_trips() {
        let encoding = Encoding::UNREAL_G16R16;
        for velocity in [-1.0_f32, -0.25, 0.0, 0.25, 1.0] {
            let stored = velocity / encoding.scale + encoding.bias;
            assert!((encoding.decode(stored) - velocity).abs() < 1e-4);
        }
    }

    #[test]
    fn unreal_bias_matches_the_engine_constant() {
        // 32767 rather than 32768. The measured mean of written pixels in the game agreed with
        // this to seven decimal places, so an off-by-one here would be visible.
        assert!((Encoding::UNREAL_G16R16.bias - 0.499_992_37).abs() < 1e-7);
    }

    #[test]
    fn unreal_scale_is_four_not_two() {
        // A decode fitted to captured data suggested two. The engine says 1/(0.499*0.5).
        assert!((Encoding::UNREAL_G16R16.scale - 4.008_016).abs() < 1e-4);
    }

    #[test]
    fn composition_is_needed_only_when_nobody_reconstructs() {
        let unreal = MotionVectors::unreal_object_only();
        assert!(
            unreal.needs_composition_for(false),
            "backend cannot reconstruct, so we must"
        );
        assert!(
            !unreal.needs_composition_for(true),
            "backend reconstructs, so we must not"
        );
    }
}
