// SPDX-License-Identifier: MIT
use crate::overlay_abi::{
    RSF_OVERLAY_FG_RUNTIME_SWITCH, RSF_SR_DLSS, RSF_SR_FSR3, RSF_SR_FSR4, RSF_SR_SPATIAL,
    rsf_native_input_routes, rsf_native_latency_status, rsf_overlay_intent as Intent,
    rsf_overlay_stats as Stats,
};
use crate::render_abi::{
    RSF_FG_OFF, RSF_FG_STAT_TOTAL_PRESENTED, RSF_SR_FSR2, RSF_SR_XESS,
    rsf_native_fg_status as FgStatus, rsf_render_settings as Settings,
    rsf_sharpening_options as Sharpening,
};
use crate::{
    cpu_abi::rsf_native_input_receipts,
    d3d11_present::{
        rsf_core_d3d11p_backend, rsf_core_d3d11p_backend_choices, rsf_core_d3d11p_has_owner,
        rsf_core_d3d11p_switch_result,
    },
    fg_choice::rsf_fg_choice_get,
    native_cpu::rsf_native_input_receipts_get,
    native_fg_cpu::rsf_core_native_input_routes_get,
};
use crate::{settings_store::set_sharpening, sharpening_policy};
use rsf_game_sys::d3d11_sr_admission_abi::RSF_SR_AUTO;
use std::ffi::CStr;

pub(super) fn fill_latency_stats(
    stats: &mut Stats,
    get_latency: unsafe extern "C" fn(*mut rsf_native_latency_status) -> i32,
    get_routes: unsafe extern "C" fn(*mut rsf_native_input_routes) -> i32,
) {
    let mut latency = rsf_native_latency_status {
        struct_size: size_of::<rsf_native_latency_status>() as u32,
        version: 1,
        ..Default::default()
    };
    if unsafe { get_latency(&mut latency) } != 0 {
        stats.pcl_available = latency.pcl_available;
    }
    let mut receipts = rsf_native_input_receipts {
        struct_size: size_of::<rsf_native_input_receipts>() as u32,
        version: 1,
        ..unsafe { std::mem::zeroed() }
    };
    if unsafe { rsf_native_input_receipts_get(&mut receipts) } != 0 {
        stats.input_receipts = receipts.input_kinds;
    }
    let mut routes = rsf_native_input_routes {
        struct_size: size_of::<rsf_native_input_routes>() as u32,
        version: 1,
        ..Default::default()
    };
    if unsafe { get_routes(&mut routes) } != 0 {
        stats.pcl_routed = routes.routed;
        stats.pcl_failed = routes.failed;
        stats.pcl_unmatched = routes.unmatched;
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_core_overlay_fill_fg_stats(
    stats: &mut Stats,
    fg: *const FgStatus,
    get_latency: unsafe extern "C" fn(*mut rsf_native_latency_status) -> i32,
) {
    fill_latency_stats(stats, get_latency, rsf_core_native_input_routes_get);
    let choice = rsf_fg_choice_get();
    stats.fg_backend = if rsf_core_d3d11p_has_owner() != 0 {
        rsf_core_d3d11p_backend()
    } else {
        0
    };
    stats.fg_requested_backend = choice.backend;
    stats.fg_backend_choices =
        (choice.choices & rsf_core_d3d11p_backend_choices()) | RSF_OVERLAY_FG_RUNTIME_SWITCH;
    stats.fg_selection_result = if choice.last_result != 0 {
        choice.last_result
    } else {
        rsf_core_d3d11p_switch_result()
    };
    let Some(fg) = (unsafe { fg.as_ref() }) else {
        return;
    };
    stats.fg_available = fg.available;
    stats.fg_requested_mode = fg.requested.mode;
    stats.fg_requested_generated = fg.requested.generated_frames;
    stats.fg_effective_mode = fg.vendor.effective_mode;
    stats.fg_effective_generated = fg.vendor.effective_generated_frames;
    stats.fg_active = fg.vendor.active;
    stats.fg_max_generated = fg.vendor.max_generated_frames;
    stats.fg_mfg_unlock_status = fg.vendor.mfg_unlock_status;
    stats.reflex_available = fg.vendor.low_latency_available;
    stats.latency_pacing_owner = fg.vendor.pacing_owner;
    stats.latency_paced_frames = fg.vendor.latency_paced_frames;
    stats.reflex_requested_mode = fg.requested.reflex_mode;
    stats.reflex_effective_mode = fg.vendor.effective_reflex;
    stats.fg_reason = fg.reason;
    stats.fg_last_result = fg.last_result;
    stats.fg_total_presented = fg.vendor.total_presented;
    stats.fg_present_count_valid = u32::from(
        fg.available != 0 && fg.vendor.valid_statistics & RSF_FG_STAT_TOTAL_PRESENTED != 0,
    );
    stats.frame_limit_us = fg.requested.frame_limit_us;
}

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

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_fill_sharpening_stats(stats: &mut Stats, options: &Sharpening) {
    stats.sharpening_available = 1;
    fill_sharpening_stats(stats, options);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_overlay_requested_sharpening(intent: &Intent) -> Sharpening {
    intent_sharpening(intent)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharpening_stats_and_intent_preserve_all_fields() {
        let original = Sharpening {
            mode: 3,
            sharpness: 0.67,
            mas: 1,
            motion_sharpness: -0.2,
            motion_threshold: 1.23,
            motion_scale_limit: 42.0,
            contrast_enabled: 1,
            contrast: 0.9,
            depth_scale: 38.0,
            depth_bias: 0.0003,
            clamp_output: 1,
            debug: 1,
            ..sharpening_policy::defaults()
        };
        let mut stats = Stats::default();
        rsf_overlay_fill_sharpening_stats(&mut stats, &original);
        assert_eq!(stats.sharpening_available, 1);
        let mut intent = Intent::default();
        macro_rules! check_fields {
            ($($field:ident => $wire:ident),+ $(,)?) => {
                $(
                    assert_eq!(stats.$wire, original.$field);
                    intent.$wire = stats.$wire;
                )+
                let mapped = rsf_overlay_requested_sharpening(&intent);
                $(assert_eq!(mapped.$field, original.$field);)+
            };
        }
        check_fields!(
            mode => sharpening_mode,
            sharpness => sharpening_sharpness,
            mas => sharpening_mas,
            motion_sharpness => sharpening_motion_sharpness,
            motion_threshold => sharpening_motion_threshold,
            motion_scale_limit => sharpening_motion_scale_limit,
            contrast_enabled => sharpening_contrast_enabled,
            contrast => sharpening_contrast,
            depth_scale => sharpening_depth_scale,
            depth_bias => sharpening_depth_bias,
            clamp_output => sharpening_clamp_output,
            debug => sharpening_debug,
        );
    }
}
