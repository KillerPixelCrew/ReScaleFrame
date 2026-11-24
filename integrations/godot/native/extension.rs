// SPDX-License-Identifier: MIT
//! `ReScaleFrameRuntime` control policy for the GDExtension class in `gdextension/runtime.rs`: the
//! option schema of `configure(options, viewport)` with its type, range and default rules, and
//! the renderer admission both `configure` and `get_status` report. The class keeps the
//! `Dictionary`, `Variant`, `RID` and `RenderingServer` calls.
use crate::control::default_settings;
use crate::platform::{OptionField, OptionValue, Platform, Settings};
use crate::properties::Properties;
use std::ffi::CStr;

pub(crate) const INTEGER: u32 = 0;
pub(crate) const REAL: u32 = 1;
pub(crate) const VARIANT_INT: u32 = 1;
pub(crate) const VARIANT_BOOL: u32 = 2;
pub(crate) const ADMIT_DRIVER: u32 = 1;
pub(crate) const ADMIT_RENDERER: u32 = 2;

pub(crate) const FIELD_COUNT: usize = 21;
// The first CHECKED fields must be Variant INT or BOOL within 0..=u32::MAX. The sharpening
// fields are converted by Godot's Variant rules without a type check.
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
/// fields keep the defaults; present integers truncate to 32 bits and reals narrow to `f32`.
pub(crate) fn compose(values: &[OptionValue; FIELD_COUNT]) -> Option<Settings> {
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
    let mut sharpening = Platform.sharpening_defaults();
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

/// Compose the options `read` fills from a borrowed Godot Dictionary, then call the registered
/// engine service with the typed settings. Only the leaf can interpret Variants.
#[cfg(all(windows, not(test)))]
pub(crate) fn configure(
    admitted: u32,
    viewport: u64,
    mut read: impl FnMut(&OptionField, &mut OptionValue),
) -> i32 {
    let required = ADMIT_DRIVER | ADMIT_RENDERER;
    if admitted & required != required {
        return 0;
    }
    let mut values = [OptionValue::default(); FIELD_COUNT];
    for (field, value) in FIELDS.0.iter().zip(&mut values) {
        read(field, value);
    }
    compose(&values).map_or(0, |value| crate::CONTROL.configure_engine(&value, viewport))
}

/// The scripting status schema. Godot's callback materializes the scalar values
/// as Variants in its own Dictionary.
// Called only by the gdextension entry points, which unit tests do not build.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn status(admitted: u32, fields: &mut Properties) {
    use crate::platform::{ABI_VERSION, SETTINGS_ABI_VERSION, Status};
    fields.driver(c"graphics_driver");
    fields.renderer(c"renderer");
    fields.boolean(c"graphics_driver_supported", admitted & ADMIT_DRIVER != 0);
    fields.boolean(c"renderer_supported", admitted & ADMIT_RENDERER != 0);
    let mut status = Status {
        struct_size: size_of::<Status>() as u32,
        abi_version: ABI_VERSION,
        ..Default::default()
    };
    let available = unsafe { crate::rsfg_status(&mut status) } != 0 && status.session_id != 0;
    fields.boolean(c"engine_bridge_ready", available);
    if !available {
        return;
    }

    let mut requested = Settings {
        struct_size: size_of::<Settings>() as u32,
        abi_version: SETTINGS_ABI_VERSION,
        ..Default::default()
    };
    if unsafe { crate::rsfg_settings(&mut requested) } != 0 {
        fields.integer(c"requested_dlss_preset", i64::from(requested.dlss_preset));
        fields.integer(c"requested_fg_backend", i64::from(requested.fg_backend));
        fields.integer(c"requested_fg_mode", i64::from(requested.fg_mode));
        fields.integer(
            c"requested_generated_frames",
            i64::from(requested.generated_frames),
        );
        fields.integer(c"requested_latency_mode", i64::from(requested.latency_mode));
    }

    macro_rules! c_key {
        ($field:ident) => {
            CStr::from_bytes_with_nul(concat!(stringify!($field), "\0").as_bytes()).unwrap()
        };
    }
    macro_rules! fields {
        ($($field:ident),* $(,)?) => {
            $(fields.integer(c_key!($field), status.$field as i64);)*
        };
    }
    fields!(
        session_id,
        requested_sr,
        active_sr,
        render_width,
        render_height,
        output_width,
        output_height,
        sr_result,
        fg_result,
        fg_backend,
        fg_available,
        fg_active,
        max_generated_frames,
        generated_frames,
        latency_available,
        latency_mode,
        reason,
        pcl_available,
        latency_report_available,
        evaluated_frames,
        refused_frames,
        tagged_frames,
        sleep_calls,
        marker_calls,
        total_presented,
        generated_presented,
        presentation_statistics_valid,
        presentation_statistics_flags,
        source_frame_id,
        provider_generation,
        generated_callbacks,
        window_foreground,
        window_visible,
        foreground_process_id,
        latency_owner,
        input_substitutes,
        order_violations,
        derived_boundaries,
        keyboard_inputs,
        mouse_inputs,
        controller_inputs,
        pcl_pings,
    );
}

#[cfg(test)]
mod tests;
