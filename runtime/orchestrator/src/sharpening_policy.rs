// SPDX-License-Identifier: MIT

use crate::sys::{RSF_SHARPENING_ABI_VERSION, rsf_sharpening_options as Options};

const VENDOR: u32 = 4;
const RCAS: u32 = 1;
const DEPTH_RCAS: u32 = 2;
const DEPTH_DAS: u32 = 3;
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

pub(crate) const fn defaults() -> Options {
    Options {
        struct_size: size_of::<Options>() as u32,
        abi_version: RSF_SHARPENING_ABI_VERSION,
        mode: crate::sys::RSF_SHARPENING_DEFAULT_MODE,
        sharpness: crate::sys::RSF_SHARPENING_DEFAULT_SHARPNESS as f32,
        mas: crate::sys::RSF_SHARPENING_DEFAULT_MAS,
        motion_sharpness: crate::sys::RSF_SHARPENING_DEFAULT_MOTION_SHARPNESS as f32,
        motion_threshold: crate::sys::RSF_SHARPENING_DEFAULT_MOTION_THRESHOLD as f32,
        motion_scale_limit: crate::sys::RSF_SHARPENING_DEFAULT_MOTION_SCALE_LIMIT as f32,
        contrast_enabled: crate::sys::RSF_SHARPENING_DEFAULT_CONTRAST_ENABLED,
        contrast: crate::sys::RSF_SHARPENING_DEFAULT_CONTRAST as f32,
        depth_scale: crate::sys::RSF_SHARPENING_DEFAULT_DEPTH_SCALE as f32,
        depth_bias: crate::sys::RSF_SHARPENING_DEFAULT_DEPTH_BIAS as f32,
        clamp_output: crate::sys::RSF_SHARPENING_DEFAULT_CLAMP_OUTPUT,
        debug: crate::sys::RSF_SHARPENING_DEFAULT_DEBUG,
    }
}

// Use the same ranges for strict admission and stored-value recovery.
macro_rules! options_policy {
    (
        integers { $( $integer:ident <= $max:expr ),+ $(,)? }
        decimals { $( $decimal:ident : ($low:expr, $high:expr) ),+ $(,)? }
    ) => {
        pub(crate) fn fields_are_zero(o: &Options) -> bool {
            true $(&& o.$integer == 0)+ $(&& o.$decimal == 0.0)+
        }

        pub(crate) fn valid(o: &Options) -> bool {
            o.struct_size as usize >= size_of::<Options>()
                && o.abi_version == RSF_SHARPENING_ABI_VERSION
                $(&& o.$integer <= $max)+
                $(&& ($low..=$high).contains(&o.$decimal))+
        }

        pub(crate) fn sanitize(o: Options) -> Options {
            let defaults = defaults();
            Options {
                $($integer: if o.$integer <= $max { o.$integer } else { defaults.$integer },)+
                $($decimal: if o.$decimal.is_finite() {
                    o.$decimal.clamp($low, $high)
                } else {
                    defaults.$decimal
                },)+
                ..defaults
            }
        }
    };
}

options_policy! {
    integers {
        mode <= VENDOR,
        mas <= 1,
        contrast_enabled <= 1,
        clamp_output <= 1,
        debug <= 1,
    }
    decimals {
        sharpness: (0.0, 1.0),
        motion_sharpness: (-1.0, 1.0),
        motion_threshold: (0.0, 100.0),
        motion_scale_limit: (0.01, 100.0),
        contrast: (0.0, 2.0),
        depth_scale: (0.0, 600.0),
        depth_bias: (0.0, 1.0),
    }
}

pub(crate) fn vendor_sharpness(o: &Options, backend: u32) -> f32 {
    if o.mode == VENDOR && (2..=4).contains(&backend) {
        o.sharpness
    } else {
        0.0
    }
}

pub(crate) fn ffx_sharpening(request: f32) -> (bool, f32) {
    let amount = if request.is_finite() {
        request.clamp(0.0, 1.0)
    } else {
        0.0
    };
    (amount > 0.0, amount)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_ffx_sharpening(request: f32, enabled: &mut bool, amount: &mut f32) {
    (*enabled, *amount) = ffx_sharpening(request);
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_core_defaults() -> Options {
    defaults()
}

// The by-value wrappers receive a complete options record even for a mismatched ABI prefix.
#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_core_sanitize(options: Options) -> Options {
    sanitize(options)
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_core_valid(options: Options) -> u32 {
    u32::from(valid(&options))
}

#[unsafe(no_mangle)]
extern "C" fn rsf_sharpening_core_vendor_sharpness(options: Options, backend: u32) -> f32 {
    vendor_sharpness(&options, backend)
}
