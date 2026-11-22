// SPDX-License-Identifier: MIT
use crate::render_abi::{RSF_SR_FSR2, RSF_SR_XESS, rsf_sharpening_options as Options};

const RCAS: u32 = 1;
const DEPTH_RCAS: u32 = 2;
const DEPTH_DAS: u32 = 3;
const VENDOR: u32 = 4;
pub(super) const FILTERS: [u32; 3] = [RCAS, DEPTH_RCAS, DEPTH_DAS];

// Modes that need depth use the same RCAS fallback on each graphics API.
pub(super) fn effective_mode(mode: u32, has_depth: bool) -> Option<u32> {
    match mode {
        RCAS => Some(RCAS),
        DEPTH_RCAS | DEPTH_DAS if has_depth => Some(mode),
        DEPTH_RCAS | DEPTH_DAS => Some(RCAS),
        _ => None,
    }
}

fn bounded(value: f32, minimum: f32, maximum: f32, default: f32) -> f32 {
    if value.is_finite() {
        value.clamp(minimum, maximum)
    } else {
        default
    }
}

fn option(value: u32, maximum: u32, default: u32) -> u32 {
    if value <= maximum { value } else { default }
}

fn sanitize(options: Options, defaults: &Options) -> Options {
    Options {
        struct_size: defaults.struct_size,
        abi_version: defaults.abi_version,
        mode: option(options.mode, VENDOR, defaults.mode),
        sharpness: bounded(options.sharpness, 0.0, 1.0, defaults.sharpness),
        mas: option(options.mas, 1, defaults.mas),
        motion_sharpness: bounded(
            options.motion_sharpness,
            -1.0,
            1.0,
            defaults.motion_sharpness,
        ),
        motion_threshold: bounded(
            options.motion_threshold,
            0.0,
            100.0,
            defaults.motion_threshold,
        ),
        motion_scale_limit: bounded(
            options.motion_scale_limit,
            0.01,
            100.0,
            defaults.motion_scale_limit,
        ),
        contrast_enabled: option(options.contrast_enabled, 1, defaults.contrast_enabled),
        contrast: bounded(options.contrast, 0.0, 2.0, defaults.contrast),
        depth_scale: bounded(options.depth_scale, 0.0, 600.0, defaults.depth_scale),
        depth_bias: bounded(options.depth_bias, 0.0, 1.0, defaults.depth_bias),
        clamp_output: option(options.clamp_output, 1, defaults.clamp_output),
        debug: option(options.debug, 1, defaults.debug),
    }
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sharpening_sanitize(options: &mut Options, defaults: &Options) {
    *options = sanitize(*options, defaults);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sharpening_valid(options: &Options, defaults: &Options) -> u32 {
    if options.struct_size < defaults.struct_size || options.abi_version != defaults.abi_version {
        return 0;
    }
    let valid = sanitize(*options, defaults);
    u32::from(
        options.mode == valid.mode
            && options.sharpness == valid.sharpness
            && options.mas == valid.mas
            && options.motion_sharpness == valid.motion_sharpness
            && options.motion_threshold == valid.motion_threshold
            && options.motion_scale_limit == valid.motion_scale_limit
            && options.contrast_enabled == valid.contrast_enabled
            && options.contrast == valid.contrast
            && options.depth_scale == valid.depth_scale
            && options.depth_bias == valid.depth_bias
            && options.clamp_output == valid.clamp_output
            && options.debug == valid.debug,
    )
}

#[unsafe(no_mangle)]
extern "C" fn rsf_core_sharpening_vendor_amount(options: &Options, backend: u32) -> f32 {
    if options.mode == VENDOR && (RSF_SR_FSR2..RSF_SR_XESS).contains(&backend) {
        options.sharpness
    } else {
        0.0
    }
}
