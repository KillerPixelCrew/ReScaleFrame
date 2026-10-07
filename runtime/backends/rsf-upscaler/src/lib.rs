//! Vendor-independent super-resolution inputs, capabilities, unit conversions, and compatibility
//! checks. This crate has no graphics API or SDK dependency, so the rules can be used without a GPU.

pub mod frame;
pub mod motion;

pub use frame::{Jitter, MotionToPixels, recommended_phase_count};
pub use motion::{ClearBehaviour, Encoding, MotionVectors};

/// A vendor's reconstruction family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    /// NVIDIA, reached through Streamline.
    Nvidia,
    /// Intel XeSS.
    Intel,
    /// AMD FidelityFX.
    Amd,
}

/// How aggressively to reconstruct. The ratio is render size to output size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Render at output resolution. Antialiasing without upscaling.
    Native,
    /// Model scale of 1/1.5 per output axis.
    Quality,
    /// Model scale of 1/1.7 per output axis.
    Balanced,
    /// Half the output extent per axis.
    Performance,
    /// One third of the output extent per axis.
    UltraPerformance,
}

impl Quality {
    /// Approximate linear scale applied to each output axis in this pure model.
    /// Live adapters query their SDK's optimal dimensions rather than using these ratios.
    #[must_use]
    pub fn scale(self) -> f32 {
        match self {
            Self::Native => 1.0,
            Self::Quality => 1.0 / 1.5,
            Self::Balanced => 1.0 / 1.7,
            Self::Performance => 0.5,
            Self::UltraPerformance => 1.0 / 3.0,
        }
    }

    /// The render size this quality asks for, given an output size.
    ///
    /// Rounds each axis to the nearest pixel and clamps it to at least one, including zero output.
    /// This is a model sizing helper; callers validate actual output dimensions separately.
    #[must_use]
    pub fn render_size(self, output: Extent) -> Extent {
        let scale = self.scale();
        Extent {
            width: (((output.width as f32) * scale).round() as u32).max(1),
            height: (((output.height as f32) * scale).round() as u32).max(1),
        }
    }
}

/// A pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    /// Horizontal pixel count; zero is representable and may require caller validation.
    pub width: u32,
    /// Vertical pixel count.
    pub height: u32,
}

impl Extent {
    /// Construct an extent without validation or clamping.
    #[must_use]
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// What a game can actually provide, established by measurement rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameInputs {
    /// Size the scene is rendered at.
    pub render: Extent,
    /// Size the result must be presented at.
    pub output: Extent,
    /// How the game's motion vectors are stored.
    pub motion: MotionVectors,
    /// Whether per-frame projection jitter is available for temporal reconstruction.
    pub jitter: bool,
    /// Whether explicit exposure is available. This model does not reject its absence;
    /// the live provider configuration decides whether auto exposure is permitted.
    pub exposure: bool,
    /// Whether the depth input required by this model is available.
    pub depth: bool,
}

/// What a backend is able to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Provider family described by these flags.
    pub vendor: Vendor,
    /// Whether the backend can build camera motion itself from depth and a previous-frame
    /// transform, given a value marking untouched pixels.
    pub reconstructs_camera_motion: bool,
    /// Whether it can run on the graphics API in use without a bridge to another one.
    pub native_api: bool,
}

/// Why a pairing will not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unusable {
    /// No jittered projection, so there is nothing extra to reconstruct from.
    NoJitter,
    /// No depth buffer.
    NoDepth,
    /// The output is smaller than the render size, which is not upscaling.
    OutputNotLarger,
    /// The backend needs a complete motion field and the game supplies object motion only, so a
    /// composition pass has to exist before this pairing is viable.
    MotionNeedsComposition,
    /// Storage has a bias that requires a decode pass before the vendor's multiplicative scale.
    /// This can apply independently of [`Unusable::MotionNeedsComposition`].
    MotionNeedsDecode,
}

/// A backend the orchestrator can drive.
pub trait Backend {
    /// Return the provider's measured/configured input capabilities.
    fn capabilities(&self) -> Capabilities;
}

/// Whether a backend can be driven from these inputs, and why not when it cannot.
///
/// Reports every modeled blocker in declaration/check order. Equal render/output extents are
/// allowed for native AA. This function does not query hardware, validate zero/finite dimensions,
/// or enforce `native_api`/exposure availability; live session creation performs those checks.
pub fn check(inputs: &GameInputs, capabilities: &Capabilities) -> Result<(), Reasons> {
    let mut reasons = Reasons::default();
    if !inputs.jitter {
        reasons.push(Unusable::NoJitter);
    }
    if !inputs.depth {
        reasons.push(Unusable::NoDepth);
    }
    if inputs.output.width < inputs.render.width || inputs.output.height < inputs.render.height {
        reasons.push(Unusable::OutputNotLarger);
    }
    if inputs
        .motion
        .needs_composition_for(capabilities.reconstructs_camera_motion)
    {
        reasons.push(Unusable::MotionNeedsComposition);
    }
    if inputs.motion.needs_decode() {
        reasons.push(Unusable::MotionNeedsDecode);
    }
    if reasons.is_empty() {
        Ok(())
    } else {
        Err(reasons)
    }
}

/// The set of reasons a pairing is unusable. Fixed capacity, so reporting a failure never
/// allocates on a path that may already be failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reasons {
    entries: [Option<Unusable>; 5],
    count: usize,
}

impl Reasons {
    // Capacity equals the five checks above. Additional blockers require extending entries too.
    fn push(&mut self, reason: Unusable) {
        if self.count < self.entries.len() {
            self.entries[self.count] = Some(reason);
            self.count += 1;
        }
    }

    /// Whether no modeled incompatibilities were recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether this failure set contains the specified blocker.
    #[must_use]
    pub fn contains(&self, reason: Unusable) -> bool {
        self.entries.iter().flatten().any(|entry| *entry == reason)
    }

    /// Iterate the reasons found.
    pub fn iter(&self) -> impl Iterator<Item = Unusable> + '_ {
        self.entries.iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streamline() -> Capabilities {
        Capabilities {
            vendor: Vendor::Nvidia,
            reconstructs_camera_motion: true,
            native_api: true,
        }
    }

    fn xess_like() -> Capabilities {
        Capabilities {
            vendor: Vendor::Intel,
            reconstructs_camera_motion: false,
            native_api: true,
        }
    }

    /// Ace Combat 7 as measured, once jitter has been re-enabled.
    fn ac7() -> GameInputs {
        GameInputs {
            render: Extent::new(1024, 576),
            output: Extent::new(2048, 1152),
            motion: MotionVectors::unreal_object_only(),
            jitter: true,
            exposure: true,
            depth: true,
        }
    }

    #[test]
    fn ac7_needs_its_motion_decoded_even_for_streamline() {
        // Camera reconstruction does not remove Unreal's biased storage encoding.
        let reasons = check(&ac7(), &streamline()).expect_err("the raw target carries a bias");
        assert!(reasons.contains(Unusable::MotionNeedsDecode));
        assert!(!reasons.contains(Unusable::MotionNeedsComposition));
    }

    #[test]
    fn ac7_can_drive_streamline_once_a_pass_has_decoded_the_motion() {
        let inputs = GameInputs {
            motion: MotionVectors::unreal_object_only().decoded(),
            ..ac7()
        };
        assert!(check(&inputs, &streamline()).is_ok());
    }

    #[test]
    fn ac7_needs_a_composition_pass_for_a_backend_that_cannot_reconstruct() {
        let reasons = check(&ac7(), &xess_like()).expect_err("object-only motion is not enough");
        assert!(reasons.contains(Unusable::MotionNeedsComposition));
    }

    #[test]
    fn no_jitter_is_refused() {
        // This was Ace Combat 7 before the anti-aliasing gate was patched.
        let inputs = GameInputs {
            jitter: false,
            ..ac7()
        };
        assert!(
            check(&inputs, &streamline())
                .unwrap_err()
                .contains(Unusable::NoJitter)
        );
    }

    #[test]
    fn every_reason_is_reported_not_just_the_first() {
        let inputs = GameInputs {
            jitter: false,
            depth: false,
            ..ac7()
        };
        let reasons = check(&inputs, &xess_like()).unwrap_err();
        assert!(reasons.contains(Unusable::NoJitter));
        assert!(reasons.contains(Unusable::NoDepth));
        assert!(reasons.contains(Unusable::MotionNeedsComposition));
        assert!(reasons.contains(Unusable::MotionNeedsDecode));
        assert_eq!(reasons.iter().count(), 4);
    }

    #[test]
    fn rendering_at_output_size_is_allowed() {
        // Native AA accepts equal sizes; this guards the former strictly-larger output check.
        let inputs = GameInputs {
            render: Extent::new(2048, 1152),
            motion: MotionVectors::unreal_object_only().decoded(),
            ..ac7()
        };
        assert!(check(&inputs, &streamline()).is_ok());
    }

    #[test]
    fn output_smaller_than_render_is_refused() {
        let inputs = GameInputs {
            output: Extent::new(512, 288),
            ..ac7()
        };
        assert!(
            check(&inputs, &streamline())
                .unwrap_err()
                .contains(Unusable::OutputNotLarger)
        );
    }

    #[test]
    fn quality_render_sizes_are_sane() {
        let output = Extent::new(2048, 1152);
        assert_eq!(Quality::Native.render_size(output), output);
        assert_eq!(
            Quality::Performance.render_size(output),
            Extent::new(1024, 576)
        );
        // Never zero, however small the output.
        assert_eq!(
            Quality::UltraPerformance.render_size(Extent::new(1, 1)),
            Extent::new(1, 1)
        );
    }
}
