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
    /// Admitted maximum generated count, including a verified RTX 40 unlock.
    pub max_generated: u32,
    /// RTX 40 MFG unlock admission, independent of observed activity.
    pub mfg_unlock_status: u32,
    /// SDK-reported low-latency availability (Reflex or XeLL).
    pub reflex_available: bool,
    /// Pacing owner: 0 none, 1 NVIDIA Reflex, 2 Intel XeLL.
    pub latency_owner: u32,
    /// Frames the pacing owner has paced.
    pub latency_paced_frames: u64,
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
    /// Requested DLSS model.
    pub dlss_preset: u32,
    /// The runtime implements DLSS model selection.
    pub dlss_preset_available: bool,
    /// The host cannot change the engine's render resolution.
    pub sr_quality_locked: bool,
    /// Whether reconstruction is currently enabled.
    pub enabled: bool,
    /// Effective SR backend.
    pub backend: u32,
    /// SR backend IDs the host can switch to, as bits. Zero means the host offers no super
    /// resolution: the panel draws no enable toggle, backend or quality choice.
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
    /// Options the host defines, in the host's order. `None` past the last one.
    pub host_options: [Option<HostOption<'a>>; MAX_HOST_OPTIONS],
    /// Host implements shared sharpening.
    pub sharpening_available: bool,
    /// The current frame has a depth input.
    pub sharpening_depth_available: bool,
    /// Current shared sharpening settings.
    pub sharpening: Sharpening,
    /// Scene render size the runtime reports, zero when unknown.
    pub render_size: [u32; 2],
    /// Output size the runtime reports, zero when unknown.
    pub output_size: [u32; 2],
    /// Saved panel layout, as the host last stored it. See [`UiPreferences`].
    pub ui_preferences: u32,
}

/// How large the panel is drawn, on top of the display-height heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelSize {
    /// The display-height heuristic alone.
    #[default]
    Auto,
    /// 0.8 times the heuristic, for a mouse on a desktop display.
    Compact,
    /// 1.25 times, for small handheld screens.
    Large,
    /// 1.5 times.
    Larger,
}

impl PanelSize {
    /// Every size, in the order the panel offers them.
    pub const ALL: [Self; 4] = [Self::Auto, Self::Compact, Self::Large, Self::Larger];

    /// Factor applied to the display-height scale.
    #[must_use]
    pub fn factor(self) -> f32 {
        match self {
            Self::Auto => 1.0,
            Self::Compact => 0.8,
            Self::Large => 1.25,
            Self::Larger => 1.5,
        }
    }

    /// What the panel calls this size.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Compact => "0.8×",
            Self::Large => "1.25×",
            Self::Larger => "1.5×",
        }
    }
}

/// Panel layout preferences. The host stores them as one opaque word
/// (`rsf_overlay_stats::ui_preferences`), so a new preference never needs host changes.
///
/// Bits 0-1 size, bit 2 right side, bit 3 detailed FPS overlay. Zero is every default, which is
/// what a host that never stored anything sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct UiPreferences {
    /// Panel size.
    pub size: PanelSize,
    /// Draw the panel on the right edge instead of the left.
    pub right: bool,
    /// The FPS overlay also shows the SR state and frame limit.
    pub detailed_hud: bool,
}

impl UiPreferences {
    /// Decode the host's word, ignoring bits this build does not know.
    #[must_use]
    pub fn from_bits(bits: u32) -> Self {
        Self {
            size: PanelSize::ALL[(bits & 0x3) as usize],
            right: bits & 0x4 != 0,
            detailed_hud: bits & 0x8 != 0,
        }
    }

    /// Encode for the host.
    #[must_use]
    pub fn to_bits(self) -> u32 {
        let size = PanelSize::ALL
            .iter()
            .position(|candidate| *candidate == self.size)
            .unwrap_or(0) as u32;
        size | (u32::from(self.right) << 2) | (u32::from(self.detailed_hud) << 3)
    }
}

/// How many host options the panel draws.
pub const MAX_HOST_OPTIONS: usize = 24;

/// One host-defined option.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostOption<'a> {
    /// What the control is called.
    pub label: &'a str,
    /// A toggle rather than a number.
    pub toggle: bool,
    /// Current value.
    pub value: i32,
    /// Minimum, maximum and step of a number.
    pub range: [i32; 3],
    /// A number is shown divided by this.
    pub divisor: u32,
    /// A change applies at the next start.
    pub restart: bool,
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
            dlss_preset: 0,
            dlss_preset_available: false,
            sr_quality_locked: false,
            enabled: false,
            backend: 1,
            backend_choices: 0,
            last_switch_result: 0,
            generation: GenerationStats::default(),
            application_presented_frames: 0,
            counter_clock: [0, 0],
            show_performance_hud: false,
            fg_present_count_valid: false,
            host_options: [None; MAX_HOST_OPTIONS],
            sharpening_available: false,
            sharpening_depth_available: false,
            sharpening: Sharpening::default(),
            render_size: [0, 0],
            output_size: [0, 0],
            ui_preferences: 0,
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

/// Shared sharpening values, independent of SR backend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpening {
    /// mode setting.
    pub mode: u32,
    /// sharpness setting.
    pub sharpness: f32,
    /// mas setting.
    pub mas: u32,
    /// motion_sharpness setting.
    pub motion_sharpness: f32,
    /// motion_threshold setting.
    pub motion_threshold: f32,
    /// motion_scale_limit setting.
    pub motion_scale_limit: f32,
    /// contrast_enabled setting.
    pub contrast_enabled: u32,
    /// contrast setting.
    pub contrast: f32,
    /// depth_scale setting.
    pub depth_scale: f32,
    /// depth_bias setting.
    pub depth_bias: f32,
    /// clamp_output setting.
    pub clamp_output: u32,
    /// debug setting.
    pub debug: u32,
}
impl Default for Sharpening {
    fn default() -> Self {
        Self {
            mode: 0,
            sharpness: 0.4,
            mas: 0,
            motion_sharpness: 0.2,
            motion_threshold: 0.0,
            motion_scale_limit: 10.0,
            contrast_enabled: 0,
            contrast: 0.0,
            depth_scale: 35.0,
            depth_bias: 0.001,
            clamp_output: 0,
            debug: 0,
        }
    }
}
/// What the user asked for in one frame.
///
/// The `*_changed` flags are true only in the frame the widget changed, so a caller acts once
/// instead of every frame after a click. The values are reported either way, so a caller that
/// missed a frame can still see what the panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Intent {
    /// The quality the panel shows as chosen.
    pub quality: Quality,
    /// Whether the quality changed in this frame.
    pub quality_changed: bool,
    /// The requested DLSS model changed.
    pub dlss_preset_changed: bool,
    /// Requested DLSS model ID.
    pub dlss_preset: u32,
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
    /// A host option changed.
    pub host_option_changed: bool,
    /// Which one, as an index into the host's options.
    pub host_option_index: u32,
    /// Its new value.
    pub host_option_value: i32,
    /// All sharpening fields are applied atomically.
    pub sharpening_changed: bool,
    /// Requested sharpening settings.
    pub sharpening: Sharpening,
    /// The user asked to close the panel: the close button, or a tap outside it.
    pub close_requested: bool,
    /// The panel layout preferences changed.
    pub ui_preferences_changed: bool,
    /// Requested layout preferences, encoded as [`UiPreferences::to_bits`].
    pub ui_preferences: u32,
}

impl Intent {
    /// Whether the user did anything at all this frame.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        !self.dlss_preset_changed
            && !self.quality_changed
            && !self.enabled_changed
            && !self.backend_changed
            && !self.fg_changed
            && !self.fg_backend_changed
            && !self.reflex_changed
            && !self.performance_hud_changed
            && !self.frame_limit_changed
            && !self.host_option_changed
            && !self.sharpening_changed
            && !self.close_requested
            && !self.ui_preferences_changed
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
    #[test]
    fn sharpening_defaults_and_idle_intent() {
        let d = Sharpening::default();
        assert_eq!(d.mode, 0);
        assert_eq!(d.sharpness, 0.4);
        assert_eq!(d.mas, 0);
        assert_eq!(d.motion_sharpness, 0.2);
        assert_eq!(d.motion_threshold, 0.0);
        assert_eq!(d.motion_scale_limit, 10.0);
        assert_eq!(d.contrast, 0.0);
        assert_eq!(d.depth_scale, 35.0);
        assert_eq!(d.depth_bias, 0.001);
        let mut intent = Intent::default();
        assert!(intent.is_idle());
        intent.sharpening_changed = true;
        assert!(!intent.is_idle());
    }

    #[test]
    fn ui_preferences_round_trip_and_zero_is_default() {
        assert_eq!(UiPreferences::from_bits(0), UiPreferences::default());
        for size in PanelSize::ALL {
            for right in [false, true] {
                for detailed_hud in [false, true] {
                    let value = UiPreferences {
                        size,
                        right,
                        detailed_hud,
                    };
                    assert_eq!(UiPreferences::from_bits(value.to_bits()), value);
                }
            }
        }
        // Unknown high bits from a newer build are ignored rather than misread.
        assert_eq!(
            UiPreferences::from_bits(0xffff_fff0),
            UiPreferences::default()
        );
    }
}
