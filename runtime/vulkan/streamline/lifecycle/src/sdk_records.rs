// SPDX-License-Identifier: MIT
//! Data construction for the pinned Streamline 2.14.1 call leaves.
use crate::sdk_api::{PREFERENCES, RESOURCE, TAG};
use crate::sdk_camera_defaults;
use crate::sys::{
    rsf_slvk_legacy_camera, rsf_slvk_legacy_image, rsf_slvk_sdk_constants, rsf_slvk_sdk_header,
    rsf_slvk_sdk_log, rsf_slvk_sdk_pcl_options, rsf_slvk_sdk_preferences,
    rsf_slvk_sdk_reflex_options, rsf_slvk_sdk_resource, rsf_slvk_sdk_tag, rsf_slvk_sdk_tag_batch,
};
use std::ffi::c_char;
use std::ptr;

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

pub fn reflex_options(mode: u32, interval_us: u32, thread_id: u32) -> rsf_slvk_sdk_reflex_options {
    rsf_slvk_sdk_reflex_options {
        base: rsf_slvk_sdk_header {
            next: ptr::null_mut(),
            type_lo: 0xf03a_f81a,
            type_mid: 0x6d0b,
            type_hi: 0x4902,
            type_tail: [0xa6, 0x51, 0xc4, 0x96, 0x5e, 0x21, 0x54, 0x34],
            version: 1,
        },
        mode,
        interval_us,
        optimize_markers: 0,
        hotkey: 0,
        thread_id,
    }
}

pub fn rsf_slvk_pack_pcl_options(thread_id: u32, output: &mut rsf_slvk_sdk_pcl_options) {
    *output = rsf_slvk_sdk_pcl_options {
        base: rsf_slvk_sdk_header {
            next: ptr::null_mut(),
            type_lo: 0xcfa3_2f9b,
            type_mid: 0x023c,
            type_hi: 0x420e,
            type_tail: [0x90, 0x56, 0x68, 0x32, 0xb7, 0x4f, 0x89, 0xb4],
            version: 1,
        },
        hotkey: 0, // eUsePingMessage
        thread_id,
    };
}

/// Matrix buffers borrow one live managed frame and are copied into the SDK's
/// row storage. The managed column-vector representation already has this
/// linear order when read as Streamline row-vector matrices.
pub(crate) unsafe fn rsf_slvk_pack_legacy_camera(
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
        view_to_clip: copy(view_to_clip),
        clip_to_view: copy(clip_to_view),
        clip_to_previous: copy(clip_to_previous),
        previous_to_clip: copy(previous_to_clip),
        jitter: camera.jitter,
        motion_scale: camera.motion_scale,
        position: camera.position,
        up: camera.up,
        right: camera.right,
        forward: camera.forward,
        near_plane: camera.near_plane,
        far_plane: camera.far_plane,
        fov: camera.fov,
        aspect: camera.aspect,
        reset: u8::from(camera.reset != 0),
        ..sdk_camera_defaults::constants()
    };
    unsafe { output.write(constants) };
    1
}

pub(crate) unsafe fn rsf_slvk_pack_legacy_tags(
    images: *const *const rsf_slvk_legacy_image,
    output: &mut rsf_slvk_sdk_tag_batch,
) {
    let images = unsafe { images.cast::<[*const rsf_slvk_legacy_image; 4]>().as_ref() };
    // Depth, motion vectors, HUD-less color, UI and alpha.
    for (index, kind) in [0, 1, 2, 23].into_iter().enumerate() {
        let mut tag = rsf_slvk_sdk_tag {
            base: TAG,
            type_: kind,
            lifecycle: 1, // eValidUntilPresent for an invalidation
            ..Default::default()
        };
        if let Some(image) = images.and_then(|images| unsafe { images[index].as_ref() }) {
            output.resources[index] = rsf_slvk_sdk_resource {
                base: RESOURCE,
                image: image.image as usize as *mut _,
                view: image.view as usize as *mut _,
                layout: image.layout,
                width: image.width,
                height: image.height,
                format: image.format,
                mips: 1,
                layers: 1,
                usage: image.usage,
                ..Default::default()
            };
            // The managed renderer reuses its staging images on the next frame.
            // Streamline copies them on this recording command buffer.
            tag.lifecycle = 0; // eOnlyValidNow
            tag.width = image.width;
            tag.height = image.height;
        }
        // The bridge installs the pointer to the batch's resource.
        output.tags[index] = tag;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_option_records_keep_default_ping_and_caller_thread() {
        let reflex = reflex_options(2, 8_333, 19);
        assert_eq!(
            (reflex.mode, reflex.interval_us, reflex.thread_id),
            (2, 8_333, 19)
        );
        assert_eq!((reflex.optimize_markers, reflex.hotkey), (0, 0));
        let mut pcl = rsf_slvk_sdk_pcl_options::default();
        rsf_slvk_pack_pcl_options(23, &mut pcl);
        assert_eq!((pcl.hotkey, pcl.thread_id), (0, 23));
    }

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

    #[test]
    fn tag_batch_preserves_frame_lifetimes_and_resource_roles() {
        let image = rsf_slvk_legacy_image {
            image: 40,
            view: 41,
            layout: 5,
            format: 44,
            width: 1280,
            height: 720,
            usage: 7,
        };
        let images = [ptr::from_ref(&image); 4];
        let mut batch = rsf_slvk_sdk_tag_batch::default();
        unsafe { rsf_slvk_pack_legacy_tags(images.as_ptr(), &mut batch) };
        assert_eq!(batch.tags.map(|tag| tag.type_), [0, 1, 2, 23]);
        assert_eq!(batch.tags[0].lifecycle, 0);
        assert_eq!([batch.tags[0].width, batch.tags[0].height], [1280, 720]);
        assert_eq!(batch.resources[0].image.addr(), 40);
        assert_eq!(batch.resources[0].view.addr(), 41);
        unsafe { rsf_slvk_pack_legacy_tags(ptr::null(), &mut batch) };
        assert_eq!(batch.tags.map(|tag| tag.lifecycle), [1; 4]);
        assert!(
            batch
                .tags
                .iter()
                .all(|tag| tag.resource.is_null() && tag.width == 0)
        );
    }
}
