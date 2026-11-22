// SPDX-License-Identifier: MIT
use super::sdk_records_abi::{rsf_slvk_sdk_constants, rsf_slvk_sdk_header};

pub(crate) fn constants() -> rsf_slvk_sdk_constants {
    rsf_slvk_sdk_constants {
        base: rsf_slvk_sdk_header {
            next: std::ptr::null_mut(),
            type_lo: 0xdcd3_5ad7,
            type_mid: 0x4e4a,
            type_hi: 0x4bad,
            type_tail: [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
            version: 2,
        },
        clip_to_lens: [f32::MAX; 16],
        invalid_motion: f32::MAX,
        motion_includes_camera: 1,
        min_relative_depth_separation: 40.0,
        ..Default::default()
    }
}
