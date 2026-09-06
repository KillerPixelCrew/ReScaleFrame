//! What a super resolution backend needs, what one can do, and whether a given game can feed it.
//!
//! Deliberately free of any vendor SDK and of any platform API, so the rules live somewhere they
//! can be read and tested without a GPU, a game, or Windows. Backends implement [`Backend`]; the
//! orchestrator asks this crate whether a pairing is viable and gets a reason when it is not.
//!
//! The shape of this comes from measuring one game rather than from reading marketing. Ace Combat
//! 7 supplies object-only motion vectors against a zero clear, no projection jitter until it is
//! re-enabled, and an exposure value in a one-by-one texture. Every one of those is a fact a
//! backend has to be told, and every one of them was initially assumed wrong.

pub mod motion;

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
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl Quality {
    /// Linear scale applied to each axis of the output size.
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
    /// Rounded rather than truncated, and never below one pixel, because a zero-sized render
    /// target is a crash rather than a small picture.
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
    pub width: u32,
    pub height: u32,
}

impl Extent {
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
    /// Whether the projection is jittered per frame. Without this a reconstruction has no extra
    /// sub-pixel samples to work with, so it can sharpen but cannot recover detail.
    pub jitter: bool,
    /// Whether an exposure value is available. Its absence is survivable: backends fall back to
    /// deriving exposure themselves, at some cost to quality.
    pub exposure: bool,
    /// Whether depth is available. Its absence is not survivable.
    pub depth: bool,
}

/// What a backend is able to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
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
}

/// A backend the orchestrator can drive.
pub trait Backend {
    /// What this backend can do. Reported, not assumed: a backend that is present is not
    /// necessarily usable on this device.
    fn capabilities(&self) -> Capabilities;
}

/// Whether a backend can be driven from these inputs, and why not when it cannot.
///
/// Returns every reason rather than the first, because discovering three blockers one run at a
/// time is how an afternoon disappears.
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
    entries: [Option<Unusable>; 4],
    count: usize,
}

impl Reasons {
    fn push(&mut self, reason: Unusable) {
        if self.count < self.entries.len() {
            self.entries[self.count] = Some(reason);
            self.count += 1;
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

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
    fn ac7_can_drive_streamline() {
        assert!(check(&ac7(), &streamline()).is_ok());
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
        assert_eq!(reasons.iter().count(), 3);
    }

    #[test]
    fn rendering_at_output_size_is_allowed() {
        // Native quality is antialiasing without upscaling, which is a real mode, so equal sizes
        // must pass. An earlier version of this test asserted the opposite and contradicted the
        // documentation on Quality::Native.
        let inputs = GameInputs {
            render: Extent::new(2048, 1152),
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
