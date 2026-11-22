// SPDX-License-Identifier: MIT
#[path = "../../vulkan/streamline/lifecycle/src/sdk_camera_defaults.rs"]
mod sdk_camera_defaults;
#[path = "../../vulkan/streamline/lifecycle/src/sdk_records_abi.rs"]
#[allow(dead_code)] // The other records are consumed by the standalone bridge.
mod sdk_records_abi;

use rsf_game_sys::rsf_frame_record;
pub(super) use sdk_records_abi::rsf_slvk_sdk_constants;

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
        ..sdk_camera_defaults::constants()
    }
}
