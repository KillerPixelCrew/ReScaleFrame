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

/// Requested, configured and observed frame generation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerationStats {
    /// Active presentation provider.
    pub backend: u32,
    /// Saved provider for next startup.
    pub requested_backend: u32,
    /// Implemented choices as provider ID bits.
    pub backend_choices: u32,
    /// Last persistence result.
    pub selection_result: i32,
    /// A cold presentation host exists.
    pub available: bool,
    /// Requested mode.
    pub requested_mode: u32,
    /// Requested generated count.
    pub requested_generated: u32,
    /// Configured mode after suspension policy.
    pub effective_mode: u32,
    /// Configured generated count.
    pub effective_generated: u32,
    /// SDK-observed generation activity.
    pub active: bool,
    /// SDK-supported maximum generated count.
    pub max_generated: u32,
    /// SDK-reported Reflex availability.
    pub reflex_available: bool,
    /// Requested Reflex mode.
    pub requested_reflex: u32,
    /// Effective Reflex mode.
    pub effective_reflex: u32,
    /// Suspension reason code.
    pub reason: u32,
    /// Last operation result.
    pub last_result: i32,
    /// SDK aggregate source and generated presents.
    pub total_presented: u64,
    /// Requested minimum interval between rendered frames, before generation, in microseconds.
    /// Zero is unlimited.
    pub frame_limit_us: u32,
    /// Refresh rate of the display showing the game, in millihertz. Zero when unknown.
    pub display_refresh_mhz: u32,
}

/// What the overlay was told about the session this frame.
///
/// The string fields are borrowed for the duration of the frame call, as the header says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats<'a> {
    /// Whether a backend has been loaded at all.
    pub backend_loaded: bool,
    /// Why the backend is unusable, when it is.
    pub refusal_reason: Option<&'a str>,
    /// The quality in effect, or `None` when the runtime named one this build does not know.
    pub quality: Option<Quality>,
    /// The raw quality value, kept so an unknown one can be shown as the number it was.
    pub quality_raw: abi::RsfOverlayQuality,
    /// Whether reconstruction is currently enabled.
    pub enabled: bool,
    /// Effective SR backend.
    pub backend: u32,
    /// SR backend IDs the host can switch to, as bits. Zero keeps the panel's default list.
    pub backend_choices: u32,
    /// Runtime switch result.
    pub last_switch_result: i32,
    /// Frame generation and latency selections with observed state.
    pub generation: GenerationStats,
    /// Real application Present counter.
    pub application_presented_frames: u64,
    /// Monotonic QPC sample and its frequency.
    pub counter_clock: [u64; 2],
    /// Show the compact performance overlay.
    pub show_performance_hud: bool,
    /// Whether the SDK aggregate present counter is usable.
    pub fg_present_count_valid: bool,
}

impl Default for Stats<'_> {
    /// Nothing loaded, nothing found, nothing counted. The state the runtime is actually in before
    /// it has established anything, which makes it the right starting point for a test as well.
    fn default() -> Self {
        Self {
            backend_loaded: false,
            refusal_reason: None,
            quality: Some(Quality::Native),
            quality_raw: abi::RSF_OVERLAY_QUALITY_NATIVE,
            enabled: false,
            backend: 1,
            backend_choices: 0,
            last_switch_result: 0,
            generation: GenerationStats::default(),
            application_presented_frames: 0,
            counter_clock: [0, 0],
            show_performance_hud: false,
            fg_present_count_valid: false,
        }
    }
}

impl Stats<'_> {
    /// Set the quality in effect from a raw ABI value, keeping the raw number.
    pub fn set_quality(&mut self, raw: abi::RsfOverlayQuality) {
        self.quality_raw = raw;
        self.quality = Quality::from_abi(raw);
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
    /// Whether the user chose an SR backend.
    pub backend_changed: bool,
    /// Backend requested by the user.
    pub backend: u32,
    /// FG mode or count changed.
    pub fg_changed: bool,
    /// Requested FG mode.
    pub fg_mode: u32,
    /// Requested generated count.
    pub fg_generated: u32,
    /// Request a generation provider and save the selection.
    pub fg_backend_changed: bool,
    /// Requested generation provider ID.
    pub fg_backend: u32,
    /// Reflex mode changed.
    pub reflex_changed: bool,
    /// Requested Reflex mode.
    pub reflex_mode: u32,
    /// The compact HUD toggle changed.
    pub performance_hud_changed: bool,
    /// Requested compact HUD visibility.
    pub performance_hud: bool,
    /// The frame limit changed.
    pub frame_limit_changed: bool,
    /// Requested minimum interval between rendered frames in microseconds. Zero is unlimited.
    pub frame_limit_us: u32,
}

impl Intent {
    /// Whether the user did anything at all this frame.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.quality_changed
            && !self.enabled_changed
            && !self.backend_changed
            && !self.fg_changed
            && !self.fg_backend_changed
            && !self.reflex_changed
            && !self.performance_hud_changed
            && !self.frame_limit_changed
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
    /// When false, only the optional startup hint is drawn. Widget state survives.
    pub visible: bool,
    /// Noninteractive startup hint opacity. Zero produces no hint.
    pub startup_hint_alpha: f32,
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
            startup_hint_alpha: 0.0,
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
}
