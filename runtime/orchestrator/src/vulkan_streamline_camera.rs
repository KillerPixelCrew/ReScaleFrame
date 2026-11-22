// SPDX-License-Identifier: MIT
// The Streamline record mirrors are loaded once, in streamline_sdk.
use crate::streamline_sdk::default_constants;
pub(super) use crate::streamline_sdk::{
    Constants as rsf_slvk_sdk_constants, Header as rsf_slvk_sdk_header,
    Preferences as rsf_slvk_sdk_preferences, Resource as rsf_slvk_sdk_resource,
    ResourceTag as rsf_slvk_sdk_tag,
};
use rsf_game_sys::rsf_frame_record;

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
        // The native call adapter fills the inverse under the existing
        // DirectXMath determinant rule.
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
