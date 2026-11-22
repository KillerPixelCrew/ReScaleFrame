// SPDX-License-Identifier: MIT
// The Streamline record mirrors are loaded once, in streamline_sdk.
pub(super) use crate::streamline_sdk::{
    Constants as rsf_slvk_sdk_constants, Header as rsf_slvk_sdk_header,
    Preferences as rsf_slvk_sdk_preferences, Resource as rsf_slvk_sdk_resource,
    ResourceTag as rsf_slvk_sdk_tag,
};
use crate::streamline_sdk::{RESOURCE, TAG, default_constants};
// The Streamline Vulkan call leaves' entry types, owned by rsf-streamline-vk.
pub(super) use rsf_streamline_vk::sdk_api;

use crate::dlss_native12::xm;
use crate::sys::rsf_vkfg_image;
use rsf_game_sys::rsf_frame_record;
use std::ffi::c_void;
use std::ptr;

pub(super) fn camera_constants(
    record: &rsf_frame_record,
    motion_scale: [f32; 2],
    reset: bool,
) -> rsf_slvk_sdk_constants {
    let camera = &record.camera;
    let world = &camera.view_to_world;
    rsf_slvk_sdk_constants {
        view_to_clip: camera.view_to_clip,
        clip_to_view: camera.clip_to_view,
        clip_to_previous: camera.clip_to_previous_clip,
        // The SDK call leaf fills the inverse under the DirectXMath
        // determinant rule (with_previous_to_clip).
        jitter: camera.jitter_pixels,
        motion_scale: [
            motion_scale[0] / record.render_width as f32,
            motion_scale[1] / record.render_height as f32,
        ],
        position: [world[12], world[13], world[14]],
        right: [world[0], world[1], world[2]],
        up: [world[4], world[5], world[6]],
        forward: [-world[8], -world[9], -world[10]],
        near_plane: camera.near_plane,
        far_plane: if camera.far_plane > 0.0 {
            camera.far_plane
        } else {
            f32::MAX
        },
        fov: camera.vertical_fov_radians,
        aspect: record.output_width as f32 / record.output_height as f32,
        depth_inverted: u8::from(camera.depth_inverted != 0),
        motion_jittered: u8::from(camera.motion_jittered != 0),
        reset: u8::from(reset),
        ..default_constants()
    }
}

/// The constants with `previous_to_clip` set to the `XMMatrixInverse` of
/// `clip_to_previous`. None for a nonfinite determinant or one below 1e-12
/// in magnitude, the rule of the former C++ leaves.
pub(super) fn with_previous_to_clip(
    constants: &rsf_slvk_sdk_constants,
) -> Option<rsf_slvk_sdk_constants> {
    let (inverse, determinant) = xm::inverse(&xm::load(&constants.clip_to_previous));
    if !determinant.is_finite() || determinant.abs() < 1e-12 {
        return None;
    }
    Some(rsf_slvk_sdk_constants {
        previous_to_clip: xm::flatten(&inverse),
        ..*constants
    })
}

/// A 2D Vulkan image resource with one mip and layer, as the SDK's
/// `sl::Resource(eTex2d, image, nullptr, view, layout)` plus extent, format
/// and usage.
pub(super) fn image_resource(
    image: &rsf_vkfg_image,
    layout: u32,
    usage: u32,
) -> rsf_slvk_sdk_resource {
    rsf_slvk_sdk_resource {
        base: RESOURCE,
        type_: 0, // eTex2d
        image: ptr::without_provenance_mut(image.image as usize),
        view: ptr::without_provenance_mut(image.view as usize),
        layout,
        width: image.width,
        height: image.height,
        format: image.format,
        mips: 1,
        layers: 1,
        usage,
        ..Default::default()
    }
}

/// `sl::ResourceTag(resource, type, lifecycle, extent)`. A missing extent
/// leaves the SDK's empty extent.
pub(super) fn resource_tag(
    resource: *mut c_void,
    kind: u32,
    lifecycle: u32,
    extent: Option<[u32; 2]>,
) -> rsf_slvk_sdk_tag {
    let [width, height] = extent.unwrap_or_default();
    rsf_slvk_sdk_tag {
        base: TAG,
        resource,
        type_: kind,
        lifecycle,
        width,
        height,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previous_to_clip_is_the_directxmath_inverse_and_refuses_singular_transforms() {
        let mut constants = default_constants();
        constants.clip_to_previous = [
            2.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, -2.0, 0.0, 1.0,
        ];
        let filled = with_previous_to_clip(&constants).unwrap();
        assert_eq!(
            filled.previous_to_clip,
            [
                0.5, 0.0, 0.0, 0.0, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -0.5, 0.5, 0.0, 1.0
            ]
        );
        assert_eq!(filled.view_to_clip, constants.view_to_clip);
        constants.clip_to_previous = [0.0; 16];
        assert!(with_previous_to_clip(&constants).is_none());
        constants.clip_to_previous[0] = f32::NAN;
        assert!(with_previous_to_clip(&constants).is_none());
    }

    #[test]
    fn resources_and_tags_carry_the_sdk_headers_and_extent() {
        let image = rsf_vkfg_image {
            image: 1000,
            view: 1001,
            width: 640,
            height: 360,
            format: 100,
            usage: 7,
            ..Default::default()
        };
        let resource = image_resource(&image, 5, 7);
        assert_eq!(resource.base.type_lo, 0x3a9d_70cf);
        assert_eq!((resource.image.addr(), resource.view.addr()), (1000, 1001));
        assert_eq!((resource.mips, resource.layers, resource.layout), (1, 1, 5));
        let tag = resource_tag(ptr::null_mut(), 23, 1, None);
        assert_eq!(
            (tag.base.type_lo, tag.type_, tag.width),
            (0x4c6a_5aad, 23, 0)
        );
        let tag = resource_tag(ptr::null_mut(), 0, 0, Some([640, 360]));
        assert_eq!((tag.width, tag.height), (640, 360));
    }
}
