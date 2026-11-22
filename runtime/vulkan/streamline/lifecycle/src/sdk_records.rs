// SPDX-License-Identifier: MIT
//! Data construction for the pinned Streamline 2.14.1 C++ call leaves.
use crate::sdk_records_abi::{
    rsf_slvk_legacy_camera, rsf_slvk_sdk_constants, rsf_slvk_sdk_header, rsf_slvk_sdk_log,
    rsf_slvk_sdk_preferences,
};
use std::ffi::c_char;
use std::ptr;

pub(crate) const VERSION: u64 = 0x0002_000e_0001_fedc;

const PREFERENCES: rsf_slvk_sdk_header = rsf_slvk_sdk_header {
    next: ptr::null_mut(),
    type_lo: 0x1ca1_0965,
    type_mid: 0xbf8e,
    type_hi: 0x432b,
    type_tail: [0x8d, 0xa1, 0x67, 0x16, 0xd8, 0x79, 0xfb, 0x14],
    version: 1,
};

const CONSTANTS: rsf_slvk_sdk_header = rsf_slvk_sdk_header {
    next: ptr::null_mut(),
    type_lo: 0xdcd3_5ad7,
    type_mid: 0x4e4a,
    type_hi: 0x4bad,
    type_tail: [0xa9, 0x0c, 0xe0, 0xc4, 0x9e, 0xb2, 0x3a, 0xfe],
    version: 2,
};

pub(crate) fn features(mask: u32) -> impl Iterator<Item = u32> {
    // PCL, Reflex, DLSS-G and DLSS SR. Order matches the native startup leaf.
    [4, 3, 1000, 0]
        .into_iter()
        .enumerate()
        .filter_map(move |(bit, feature)| (mask & (1 << bit) != 0).then_some(feature))
}

pub(crate) fn preferences(
    features: &[u32],
    plugin_paths: *const *const u16,
    project: *const c_char,
    verbose: bool,
    log: rsf_slvk_sdk_log,
) -> rsf_slvk_sdk_preferences {
    rsf_slvk_sdk_preferences {
        base: PREFERENCES,
        log_level: if verbose { 2 } else { 1 },
        plugin_paths: plugin_paths.cast_mut(),
        num_plugin_paths: 1,
        log,
        // eUseManualHooking | eUseFrameBasedResourceTagging
        flags: (1 << 2) | (1 << 7),
        features: features.as_ptr(),
        num_features: features.len() as u32,
        engine: 0, // eCustom
        engine_version: c"0.3".as_ptr(),
        project,
        render_api: 2, // eVulkan
        ..Default::default()
    }
}

/// Matrix buffers borrow one live managed frame and are copied into the SDK's
/// row storage. The managed column-vector representation already has this
/// linear order when read as Streamline row-vector matrices.
#[unsafe(no_mangle)]
unsafe extern "C" fn rsf_slvk_pack_legacy_camera(
    camera: *const rsf_slvk_legacy_camera,
    output: *mut rsf_slvk_sdk_constants,
) -> u32 {
    let Some(camera) = (unsafe { camera.as_ref() }) else {
        return 0;
    };
    let [
        view_to_clip,
        clip_to_view,
        clip_to_previous,
        previous_to_clip,
    ] = [
        camera.view_to_clip,
        camera.clip_to_view,
        camera.clip_to_previous,
        camera.previous_to_clip,
    ];
    if output.is_null()
        || [
            view_to_clip,
            clip_to_view,
            clip_to_previous,
            previous_to_clip,
        ]
        .iter()
        .any(|matrix| matrix.is_null())
    {
        return 0;
    }
    let copy = |matrix: *const f32| unsafe { matrix.cast::<[f32; 16]>().read() };
    let constants = rsf_slvk_sdk_constants {
        base: CONSTANTS,
        view_to_clip: copy(view_to_clip),
        clip_to_view: copy(clip_to_view),
        clip_to_lens: [f32::MAX; 16],
        clip_to_previous: copy(clip_to_previous),
        previous_to_clip: copy(previous_to_clip),
        jitter: camera.jitter,
        motion_scale: camera.motion_scale,
        pinhole: [0.0; 2],
        position: camera.position,
        up: camera.up,
        right: camera.right,
        forward: camera.forward,
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        fov: camera.fov,
        aspect: camera.aspect,
        invalid_motion: f32::MAX,
        depth_inverted: 0,
        motion_includes_camera: 1,
        motion_3d: 0,
        reset: u8::from(camera.reset != 0),
        orthographic: 0,
        motion_dilated: 0,
        motion_jittered: 0,
        min_relative_depth_separation: 40.0,
    };
    unsafe { output.write(constants) };
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_camera_keeps_matrix_storage_order_and_streamline_defaults() {
        let matrix = std::array::from_fn::<_, 16, _>(|index| index as f32);
        let mut camera = rsf_slvk_legacy_camera {
            view_to_clip: matrix.as_ptr(),
            clip_to_view: matrix.as_ptr(),
            clip_to_previous: matrix.as_ptr(),
            previous_to_clip: matrix.as_ptr(),
            jitter: [-0.25, 0.5],
            motion_scale: [0.5, -0.5],
            reset: 4,
            ..Default::default()
        };
        let mut packed = rsf_slvk_sdk_constants::default();
        assert_eq!(
            unsafe { rsf_slvk_pack_legacy_camera(&camera, &mut packed) },
            1
        );
        assert_eq!(packed.view_to_clip, matrix);
        assert_eq!(packed.previous_to_clip, matrix);
        assert_eq!(packed.clip_to_lens, [f32::MAX; 16]);
        assert_eq!(packed.jitter, camera.jitter);
        assert_eq!(packed.motion_scale, camera.motion_scale);
        assert_eq!(packed.reset, 1);
        camera.view_to_clip = ptr::null();
        assert_eq!(
            unsafe { rsf_slvk_pack_legacy_camera(&camera, &mut packed) },
            0
        );
    }
}
