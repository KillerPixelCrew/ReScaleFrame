// SPDX-License-Identifier: MIT
//! `ReScaleFrameRuntime` control policy for the GDExtension class in `runtime_control.cpp`: the
//! option schema of `configure(options, viewport)` with its type, range and default rules, and
//! the renderer admission both `configure` and `get_status` report. The class keeps the typed
//! godot-cpp `Dictionary`, `RID` and `RenderingServer` calls.
use crate::control::default_settings;
use crate::platform::{OptionField, OptionValue, Platform, Settings};
use std::ffi::CStr;

pub(crate) const INTEGER: u32 = 0;
pub(crate) const REAL: u32 = 1;
pub(crate) const VARIANT_INT: u32 = 1;
pub(crate) const VARIANT_BOOL: u32 = 2;
pub(crate) const ADMIT_DRIVER: u32 = 1;
pub(crate) const ADMIT_RENDERER: u32 = 2;

pub(crate) const FIELD_COUNT: usize = 21;
// The first CHECKED fields must be Variant INT or BOOL within 0..=u32::MAX. The sharpening
// fields are converted by Godot's Variant rules without a type check, as they always were.
const CHECKED: usize = 9;
const SR_BACKEND: usize = 0;
const QUALITY: usize = 1;
const DLSS_PRESET: usize = 2;
const FG_BACKEND: usize = 3;
const FG_MODE: usize = 4;
const GENERATED_FRAMES: usize = 5;
const LATENCY_MODE: usize = 6;
const FRAME_LIMIT_US: usize = 7;
const GAMEPLAY: usize = 8;
const SHARPENING_MODE: usize = 9;
const SHARPENING_SHARPNESS: usize = 10;
const SHARPENING_MAS: usize = 11;
const SHARPENING_MOTION_SHARPNESS: usize = 12;
const SHARPENING_MOTION_THRESHOLD: usize = 13;
const SHARPENING_MOTION_SCALE_LIMIT: usize = 14;
const SHARPENING_CONTRAST_ENABLED: usize = 15;
const SHARPENING_CONTRAST: usize = 16;
const SHARPENING_DEPTH_SCALE: usize = 17;
const SHARPENING_DEPTH_BIAS: usize = 18;
const SHARPENING_CLAMP_OUTPUT: usize = 19;
const SHARPENING_DEBUG: usize = 20;

const fn field(name: &'static CStr, conversion: u32) -> OptionField {
    OptionField {
        name: name.as_ptr(),
        conversion,
        reserved: 0,
    }
}

pub(crate) struct FieldTable(pub(crate) [OptionField; FIELD_COUNT]);
// SAFETY: the names are immutable static C strings.
unsafe impl Sync for FieldTable {}

/// Dictionary keys in index order.
pub(crate) static FIELDS: FieldTable = FieldTable([
    field(c"sr_backend", INTEGER),
    field(c"quality", INTEGER),
    field(c"dlss_preset", INTEGER),
    field(c"fg_backend", INTEGER),
    field(c"fg_mode", INTEGER),
    field(c"generated_frames", INTEGER),
    field(c"latency_mode", INTEGER),
    field(c"frame_limit_us", INTEGER),
    field(c"gameplay", INTEGER),
    field(c"sharpening_mode", INTEGER),
    field(c"sharpening_sharpness", REAL),
    field(c"sharpening_mas", INTEGER),
    field(c"sharpening_motion_sharpness", REAL),
    field(c"sharpening_motion_threshold", REAL),
    field(c"sharpening_motion_scale_limit", REAL),
    field(c"sharpening_contrast_enabled", INTEGER),
    field(c"sharpening_contrast", REAL),
    field(c"sharpening_depth_scale", REAL),
    field(c"sharpening_depth_bias", REAL),
    field(c"sharpening_clamp_output", INTEGER),
    field(c"sharpening_debug", INTEGER),
]);

/// The requested settings, or `None` when a checked field has the wrong type or range. Absent
/// fields keep the defaults; present integers truncate to 32 bits and reals narrow to `f32`, the
/// conversions the C++ class applied.
pub(crate) fn compose(
    platform: &Platform,
    values: &[OptionValue; FIELD_COUNT],
) -> Option<Settings> {
    let refused = values[..CHECKED].iter().any(|value| {
        value.present != 0
            && (!matches!(value.variant, VARIANT_INT | VARIANT_BOOL)
                || !(0..=i64::from(u32::MAX)).contains(&value.integer))
    });
    if refused {
        return None;
    }
    let integer = |index: usize, fallback: u32| {
        let value = &values[index];
        if value.present != 0 {
            value.integer as u32
        } else {
            fallback
        }
    };
    let real = |index: usize, fallback: f32| {
        let value = &values[index];
        if value.present != 0 {
            value.real as f32
        } else {
            fallback
        }
    };
    let mut settings = default_settings();
    settings.sr_backend = integer(SR_BACKEND, settings.sr_backend);
    settings.quality = integer(QUALITY, settings.quality);
    settings.dlss_preset = integer(DLSS_PRESET, settings.dlss_preset);
    settings.fg_backend = integer(FG_BACKEND, settings.fg_backend);
    settings.fg_mode = integer(FG_MODE, settings.fg_mode);
    settings.generated_frames = integer(GENERATED_FRAMES, settings.generated_frames);
    settings.latency_mode = integer(LATENCY_MODE, settings.latency_mode);
    settings.frame_limit_us = integer(FRAME_LIMIT_US, settings.frame_limit_us);
    settings.gameplay = integer(GAMEPLAY, settings.gameplay);
    let mut sharpening = platform.sharpening_defaults();
    sharpening.mode = integer(SHARPENING_MODE, sharpening.mode);
    sharpening.sharpness = real(SHARPENING_SHARPNESS, sharpening.sharpness);
    sharpening.mas = integer(SHARPENING_MAS, sharpening.mas);
    sharpening.motion_sharpness = real(SHARPENING_MOTION_SHARPNESS, sharpening.motion_sharpness);
    sharpening.motion_threshold = real(SHARPENING_MOTION_THRESHOLD, sharpening.motion_threshold);
    sharpening.motion_scale_limit =
        real(SHARPENING_MOTION_SCALE_LIMIT, sharpening.motion_scale_limit);
    sharpening.contrast_enabled = integer(SHARPENING_CONTRAST_ENABLED, sharpening.contrast_enabled);
    sharpening.contrast = real(SHARPENING_CONTRAST, sharpening.contrast);
    sharpening.depth_scale = real(SHARPENING_DEPTH_SCALE, sharpening.depth_scale);
    sharpening.depth_bias = real(SHARPENING_DEPTH_BIAS, sharpening.depth_bias);
    sharpening.clamp_output = integer(SHARPENING_CLAMP_OUTPUT, sharpening.clamp_output);
    sharpening.debug = integer(SHARPENING_DEBUG, sharpening.debug);
    settings.sharpening = sharpening;
    Some(settings)
}

/// Forward+ on Direct3D 12, or on Vulkan when this build carries the Vulkan SR payload. Anything
/// else, including a missing RenderingServer, is refused.
pub(crate) fn admission(
    driver: Option<&[u8]>,
    method: Option<&[u8]>,
    vulkan_supported: bool,
) -> u32 {
    let driver = match driver {
        Some(b"d3d12") => true,
        Some(b"vulkan") => vulkan_supported,
        _ => false,
    };
    let renderer = method == Some(b"forward_plus".as_slice());
    (if driver { ADMIT_DRIVER } else { 0 }) | (if renderer { ADMIT_RENDERER } else { 0 })
}

#[cfg(test)]
mod tests;
