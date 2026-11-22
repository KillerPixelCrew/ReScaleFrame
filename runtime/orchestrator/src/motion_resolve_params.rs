// SPDX-License-Identifier: MIT
use crate::native_sr_abi::rsf_motion_decode_params;
use crate::render_abi::rsf_motion_resolve_params;
use std::ffi::c_void;

pub(crate) fn has_motion_sentinel(decode: &rsf_motion_decode_params) -> u32 {
    u32::from(decode.zero_means_unwritten != 0 || decode.validity_enabled != 0)
}

// The decoder emits current-minus-previous displacement. Normalize to UV before
// calling this helper, which converts that displacement to previous-minus-current pixels.
pub(crate) fn dense_resolve_params(
    clip_to_previous: [f32; 16],
    extent: [u32; 2],
    motion_to_uv: [f32; 2],
    sentinel: (f32, u32),
    depth_layer: *mut c_void,
    jitter_pixels: [f32; 2],
) -> rsf_motion_resolve_params {
    rsf_motion_resolve_params {
        struct_size: size_of::<rsf_motion_resolve_params>() as u32,
        clip_to_previous,
        decoded_to_pixels: [0, 1].map(|axis| -(extent[axis] as f32) * motion_to_uv[axis]),
        sentinel: sentinel.0,
        has_sentinel: sentinel.1,
        depth_layer,
        jitter_pixels,
    }
}
