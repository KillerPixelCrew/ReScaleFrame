// SPDX-License-Identifier: MIT
use crate::sdk_api::header;
use crate::sys::rsf_slvk_sdk_constants;

pub fn constants() -> rsf_slvk_sdk_constants {
    rsf_slvk_sdk_constants {
        base: header(
            0xdcd3_5ad7,
            0x4e4a,
            0x4bad,
            [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
            2,
        ),
        clip_to_lens: [f32::MAX; 16],
        invalid_motion: f32::MAX,
        motion_includes_camera: 1,
        min_relative_depth_separation: 40.0,
        ..Default::default()
    }
}
