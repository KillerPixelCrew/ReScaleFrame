//! The overlay's data, in ordinary Rust types.
//!
//! Nothing in here holds a pointer or knows that a C boundary exists. The FFI layer converts into
//! and out of these types, which is why the panel and its tests can run without any of that.

use crate::abi;

/// How aggressively to reconstruct.
///
/// The order matches `rsf_overlay_quality` in the header and `rsf_upscaler::Quality`. A value that
/// means different things in three places is a bug waiting for someone to add a level, so
/// [`Quality::to_abi`] and [`Quality::from_abi`] are the only places the numbers appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Quality {
    /// Render at output resolution. Antialiasing without upscaling.
    #[default]
    Native,
    /// Quality preset.
    Quality,
    /// Balanced preset.
    Balanced,
    /// Performance preset.
    Performance,
    /// Ultra performance preset.
    UltraPerformance,
}

impl Quality {
    /// Every level, in the order the panel offers them.
    pub const ALL: [Self; 5] = [
        Self::Native,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
        Self::UltraPerformance,
    ];

    /// The value the C header gives this level.
    #[must_use]
    pub fn to_abi(self) -> abi::RsfOverlayQuality {
        match self {
            Self::Native => abi::RSF_OVERLAY_QUALITY_NATIVE,
            Self::Quality => abi::RSF_OVERLAY_QUALITY_QUALITY,
            Self::Balanced => abi::RSF_OVERLAY_QUALITY_BALANCED,
            Self::Performance => abi::RSF_OVERLAY_QUALITY_PERFORMANCE,
            Self::UltraPerformance => abi::RSF_OVERLAY_QUALITY_ULTRA_PERFORMANCE,
        }
    }

    /// The level a C value names, or `None` when the caller sent one this build does not know.
    ///
    /// An unknown level is shown as unknown rather than rounded to the nearest one. Silently
    /// displaying Native when the runtime is doing something else is the kind of small lie this
    /// panel exists to avoid.
    #[must_use]
    pub fn from_abi(value: abi::RsfOverlayQuality) -> Option<Self> {
        match value {
            abi::RSF_OVERLAY_QUALITY_NATIVE => Some(Self::Native),
            abi::RSF_OVERLAY_QUALITY_QUALITY => Some(Self::Quality),
            abi::RSF_OVERLAY_QUALITY_BALANCED => Some(Self::Balanced),
            abi::RSF_OVERLAY_QUALITY_PERFORMANCE => Some(Self::Performance),
            abi::RSF_OVERLAY_QUALITY_ULTRA_PERFORMANCE => Some(Self::UltraPerformance),
            _ => None,
        }
    }

    /// What the panel calls this level.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Native",
            Self::Quality => "Quality",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::UltraPerformance => "Ultra perf",
        }
    }
}

/// What the overlay was told about the session this frame.
///
/// The string fields are borrowed for the duration of the frame call, as the header says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats<'a> {
    /// Whether a backend has been loaded at all.
    pub backend_loaded: bool,
    /// Whether the driver accepted it.
    pub backend_supported: bool,
    /// Vendor name, when there is one.
    pub backend_name: Option<&'a str>,
    /// Why the backend is unusable, when it is.
    pub refusal_reason: Option<&'a str>,
    /// Size the scene is rendered at.
    pub render: [u32; 2],
    /// Size the result is presented at.
    pub output: [u32; 2],
    /// Frames presented.
    pub frames_presented: u32,
    /// Frames the backend evaluated.
    pub frames_evaluated: u32,
    /// Frames the backend refused.
    pub frames_refused: u32,
    /// The backend's own last result code.
    pub last_result: i32,
    /// Scene colour was found this frame.
    pub have_scene_color: bool,
    /// Depth was found this frame.
    pub have_depth: bool,
    /// Motion was found this frame.
    pub have_motion: bool,
    /// An exposure value was found this frame.
    pub have_exposure: bool,
    /// The motion has been decoded into something a backend can consume.
    pub motion_decoded: bool,
    /// The projection carries a jitter.
    pub jitter_active: bool,
    /// The jitter offset in pixels, x then y.
    pub jitter_pixels: [f32; 2],
    /// The quality in effect, or `None` when the runtime named one this build does not know.
    pub quality: Option<Quality>,
    /// The raw quality value, kept so an unknown one can be shown as the number it was.
    pub quality_raw: abi::RsfOverlayQuality,
    /// Whether reconstruction is currently enabled.
    pub enabled: bool,
}

impl Default for Stats<'_> {
    /// Nothing loaded, nothing found, nothing counted. The state the runtime is actually in before
    /// it has established anything, which makes it the right starting point for a test as well.
    fn default() -> Self {
        Self {
            backend_loaded: false,
            backend_supported: false,
            backend_name: None,
            refusal_reason: None,
            render: [0, 0],
            output: [0, 0],
            frames_presented: 0,
            frames_evaluated: 0,
            frames_refused: 0,
            last_result: 0,
            have_scene_color: false,
            have_depth: false,
            have_motion: false,
            have_exposure: false,
            motion_decoded: false,
            jitter_active: false,
            jitter_pixels: [0.0, 0.0],
            quality: Some(Quality::Native),
            quality_raw: abi::RSF_OVERLAY_QUALITY_NATIVE,
            enabled: false,
        }
    }
}

impl Stats<'_> {
    /// Set the quality in effect from a raw ABI value, keeping the raw number.
    pub fn set_quality(&mut self, raw: abi::RsfOverlayQuality) {
        self.quality_raw = raw;
        self.quality = Quality::from_abi(raw);
    }

    /// Render size as a fraction of output size, per axis.
    ///
    /// `None` when either output axis is zero, which is what a size the runtime has not yet
    /// established looks like. This is the number a quality level actually means, so it is derived
    /// from the two sizes rather than from the selected level.
    #[must_use]
    pub fn render_scale(&self) -> Option<[f32; 2]> {
        if self.output[0] == 0 || self.output[1] == 0 {
            return None;
        }
        Some([
            self.render[0] as f32 / self.output[0] as f32,
            self.render[1] as f32 / self.output[1] as f32,
        ])
    }

    /// Evaluated frames as a fraction of presented frames.
    ///
    /// `None` when nothing has been presented yet. A rate is a convenience here and never a
    /// replacement for the counters: the panel shows both.
    #[must_use]
    pub fn evaluated_fraction(&self) -> Option<f32> {
        if self.frames_presented == 0 {
            return None;
        }
        Some(self.frames_evaluated as f32 / self.frames_presented as f32)
    }
}

/// What the user asked for in one frame.
///
/// The `*_changed` flags are true only in the frame the widget changed, so a caller acts once
/// instead of every frame after a click. The values are reported either way, so a caller that
/// missed a frame can still see what the panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Intent {
    /// The quality the panel shows as chosen.
    pub quality: Quality,
    /// Whether the quality changed in this frame.
    pub quality_changed: bool,
    /// The enable state the panel shows.
    pub enabled: bool,
    /// Whether the enable state changed in this frame.
    pub enabled_changed: bool,
    /// Whether the user asked for this frame's inputs to be written out.
    pub dump_requested: bool,
}

impl Intent {
    /// Whether the user did anything at all this frame.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.quality_changed && !self.enabled_changed && !self.dump_requested
    }
}

/// One frame of input, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameInput {
    /// Mouse position in the presented image's coordinates.
    pub mouse: [f32; 2],
    /// Held mouse buttons, as `RSF_OVERLAY_MOUSE_*` bits.
    pub mouse_buttons: u32,
    /// Wheel movement since the previous frame.
    pub scroll_delta: f32,
    /// Size of the presented image.
    pub display: [u32; 2],
    /// Seconds since the previous frame.
    pub delta_seconds: f32,
    /// When false, nothing is laid out and no draw calls are produced. Widget state survives.
    pub visible: bool,
}

impl Default for FrameInput {
    fn default() -> Self {
        Self {
            mouse: [0.0, 0.0],
            mouse_buttons: 0,
            scroll_delta: 0.0,
            display: [1920, 1080],
            delta_seconds: 1.0 / 60.0,
            visible: true,
        }
    }
}

/// A patch for the overlay's texture atlas, with the pixels still owned by the overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureUpdate {
    /// Which texture this patches.
    pub id: u64,
    /// Destination x of the patch.
    pub x: u32,
    /// Destination y of the patch.
    pub y: u32,
    /// Patch width.
    pub width: u32,
    /// Patch height.
    pub height: u32,
    /// Tightly packed RGBA bytes, `width * height * 4` of them.
    pub pixels: Vec<u8>,
    /// Whether the destination has to be created or recreated at this size first.
    pub is_whole_texture: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header is the contract, so these are its literals rather than references to the
    /// constants this crate exports. If the two ever disagree, this test is the one that notices.
    #[test]
    fn quality_values_match_the_c_header() {
        assert_eq!(Quality::Native.to_abi(), 0);
        assert_eq!(Quality::Quality.to_abi(), 1);
        assert_eq!(Quality::Balanced.to_abi(), 2);
        assert_eq!(Quality::Performance.to_abi(), 3);
        assert_eq!(Quality::UltraPerformance.to_abi(), 4);

        for (index, level) in Quality::ALL.iter().enumerate() {
            assert_eq!(level.to_abi(), index as u32);
            assert_eq!(Quality::from_abi(index as u32), Some(*level));
        }
    }

    #[test]
    fn an_unknown_quality_stays_unknown() {
        assert_eq!(Quality::from_abi(5), None);
        assert_eq!(Quality::from_abi(u32::MAX), None);

        let mut stats = Stats::default();
        stats.set_quality(7);
        assert_eq!(stats.quality, None);
        assert_eq!(stats.quality_raw, 7);
    }

    #[test]
    fn render_scale_is_the_ratio_and_survives_an_unknown_size() {
        let stats = Stats {
            render: [1024, 576],
            output: [2048, 1152],
            ..Stats::default()
        };
        assert_eq!(stats.render_scale(), Some([0.5, 0.5]));
        assert_eq!(Stats::default().render_scale(), None);
    }

    #[test]
    fn evaluated_fraction_needs_a_presented_frame() {
        assert_eq!(Stats::default().evaluated_fraction(), None);
        let stats = Stats {
            frames_presented: 200,
            frames_evaluated: 150,
            ..Stats::default()
        };
        assert_eq!(stats.evaluated_fraction(), Some(0.75));
    }
}
