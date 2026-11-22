// SPDX-License-Identifier: MIT
use crate::overlay_abi::{
    RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL, rsf_overlay_intent as Intent,
    rsf_overlay_stats as Stats,
};
use crate::render_abi::{
    RSF_FG_OFF, RSF_SR_FSR2, RSF_SR_XESS, rsf_render_settings as Settings,
    rsf_sharpening_options as Sharpening,
};
use crate::{settings_store::set_sharpening, sharpening_policy};
use rsf_game_sys::d3d11_sr_admission_abi::RSF_SR_AUTO;
use std::ffi::CStr;

pub(super) fn sr_name(backend: u32) -> &'static CStr {
    match backend {
        RSF_SR_DLSS => c"DLSS",
        RSF_SR_FSR2 => c"FSR2",
        RSF_SR_FSR3 => c"FSR3",
        RSF_SR_FSR4 => c"FSR4",
        RSF_SR_XESS => c"XeSS",
        RSF_SR_SPATIAL => c"Spatial",
        RSF_SR_AUTO => c"Auto",
        _ => c"Off",
    }
}

pub(super) fn intent_sharpening(intent: &Intent) -> Sharpening {
    Sharpening {
        mode: intent.sharpening_mode,
        sharpness: intent.sharpening_sharpness,
        mas: intent.sharpening_mas,
        motion_sharpness: intent.sharpening_motion_sharpness,
        motion_threshold: intent.sharpening_motion_threshold,
        motion_scale_limit: intent.sharpening_motion_scale_limit,
        contrast_enabled: intent.sharpening_contrast_enabled,
        contrast: intent.sharpening_contrast,
        depth_scale: intent.sharpening_depth_scale,
        depth_bias: intent.sharpening_depth_bias,
        clamp_output: intent.sharpening_clamp_output,
        debug: intent.sharpening_debug,
        ..sharpening_policy::defaults()
    }
}

pub(super) fn fill_sharpening_stats(stats: &mut Stats, options: &Sharpening) {
    stats.sharpening_mode = options.mode;
    stats.sharpening_sharpness = options.sharpness;
    stats.sharpening_mas = options.mas;
    stats.sharpening_motion_sharpness = options.motion_sharpness;
    stats.sharpening_motion_threshold = options.motion_threshold;
    stats.sharpening_motion_scale_limit = options.motion_scale_limit;
    stats.sharpening_contrast_enabled = options.contrast_enabled;
    stats.sharpening_contrast = options.contrast;
    stats.sharpening_depth_scale = options.depth_scale;
    stats.sharpening_depth_bias = options.depth_bias;
    stats.sharpening_clamp_output = options.clamp_output;
    stats.sharpening_debug = options.debug;
}

pub(super) fn settings_changed(intent: &Intent) -> bool {
    intent.sharpening_changed != 0
        || intent.backend_changed != 0
        || intent.enabled_changed != 0
        || intent.quality_changed != 0
        || intent.dlss_preset_changed != 0
        || intent.fg_backend_changed != 0
        || intent.fg_changed != 0
        || intent.reflex_changed != 0
        || intent.frame_limit_changed != 0
        || intent.performance_hud_changed != 0
}

// Keep raw toggle values so both frontend owners can validate the whole request.
pub(super) fn apply_settings_intent(prefs: &mut Settings, intent: &Intent) {
    if intent.sharpening_changed != 0 {
        let requested = intent_sharpening(intent);
        set_sharpening(prefs, &requested);
    }
    if intent.backend_changed != 0 {
        prefs.sr_backend = intent.backend;
    }
    if intent.enabled_changed != 0 {
        prefs.sr_enabled = intent.enabled;
    }
    if intent.quality_changed != 0 {
        prefs.sr_quality = intent.quality;
    }
    if intent.dlss_preset_changed != 0 {
        prefs.dlss_preset = intent.dlss_preset;
    }
    if intent.fg_backend_changed != 0 {
        if intent.fg_backend == 0 {
            prefs.fg_enabled = 0;
        } else {
            prefs.fg_backend = intent.fg_backend;
            prefs.fg_enabled = 1;
        }
    }
    if intent.fg_changed != 0 {
        prefs.fg_enabled = u32::from(intent.fg_mode != RSF_FG_OFF);
        if intent.fg_generated != 0 {
            prefs.generated_frames = intent.fg_generated;
        }
    }
    if intent.reflex_changed != 0 {
        prefs.latency = intent.reflex_mode;
    }
    if intent.frame_limit_changed != 0 {
        prefs.frame_limit_us = intent.frame_limit_us;
    }
    if intent.performance_hud_changed != 0 {
        prefs.status_overlay = intent.performance_hud;
    }
}
