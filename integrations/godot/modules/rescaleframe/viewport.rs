// SPDX-License-Identifier: MIT
//! Private Rust object linked into Godot's engine module by SCons.
#![no_std]

mod cpu_facts;

// Settings values from sr_session.h and local mode values from viewport.h.
const SR_NONE: u32 = 0;
const SR_SPATIAL: u32 = 6;
const NATIVE: u32 = 0;
const SPATIAL: u32 = 1;
const TEMPORAL: u32 = 2;

#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_viewport_mode(requested_sr: u32) -> u32 {
    match requested_sr {
        SR_NONE => NATIVE,
        SR_SPATIAL => SPATIAL,
        _ => TEMPORAL,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rsf_godot_viewport_initial_scale(mode: u32, quality: u32) -> f32 {
    if mode == NATIVE {
        return 1.0;
    }
    // Preserve the engine's double-precision ratios before narrowing to its float setter.
    // The native host rejects unknown qualities before applying a scale.
    let scale = match quality {
        0 => 1.0_f64,
        1 => 2.0 / 3.0,
        2 => 1.0 / 1.7,
        3 => 0.5,
        4 => 1.0 / 3.0,
        5 => 0.77,
        _ => 1.0,
    };
    scale as f32
}
