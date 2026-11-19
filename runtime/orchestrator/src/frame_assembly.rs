// SPDX-License-Identifier: MIT
use crate::render_abi::*;

// Each call structure starts with its readable byte count, including the output.
unsafe fn has_capacity<T>(p: *const T) -> bool {
    !p.is_null() && (unsafe { p.cast::<u32>().read() } as usize) >= size_of::<T>()
}

#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn rsf_assemble_dlss_frame(
    camera: *const rsf_pipeline_camera_frame,
    resources: *const rsf_frame_resources,
    out: *mut rsf_dlss_frame,
) -> i32 {
    if !unsafe { has_capacity(camera) && has_capacity(resources) && has_capacity(out) } {
        return -1;
    }
    let camera = unsafe { &*camera };
    let resources = unsafe { &*resources };
    if camera.abi_version != RSF_FRAME_ASSEMBLY_ABI_VERSION {
        return -2;
    }
    if resources.color_in.is_null()
        || resources.color_out.is_null()
        || resources.depth.is_null()
        || resources.motion.is_null()
    {
        return -1;
    }
    if camera.motion_decoded == 0 {
        return -4;
    }
    if camera.has_jitter == 0
        || resources.render_width == 0
        || resources.render_height == 0
        || resources.output_width < resources.render_width
        || resources.output_height < resources.render_height
    {
        return -3;
    }
    let frame = rsf_dlss_frame {
        struct_size: unsafe { out.cast::<u32>().read() },
        abi_version: RSF_DLSS_ABI_VERSION,
        color_in: resources.color_in,
        color_out: resources.color_out,
        depth: resources.depth,
        motion: resources.motion,
        exposure: resources.exposure,
        render_width: resources.render_width,
        render_height: resources.render_height,
        output_width: resources.output_width,
        output_height: resources.output_height,
        quality: resources.quality,
        camera_view_to_clip: camera.view_to_clip,
        clip_to_camera_view: camera.clip_to_view,
        clip_to_prev_clip: camera.clip_to_prev_clip,
        prev_clip_to_clip: camera.prev_clip_to_clip,
        camera_position: camera.camera_position,
        camera_forward: camera.camera_forward,
        camera_up: camera.camera_up,
        camera_right: camera.camera_right,
        jitter_x: camera.jitter_pixels[0],
        jitter_y: camera.jitter_pixels[1],
        motion_scale_x: camera.motion_scale[0],
        motion_scale_y: camera.motion_scale[1],
        near_plane: camera.near_plane,
        // Represent an infinite far plane with the finite distance expected by the backend.
        far_plane: if camera.far_plane > 0.0 {
            camera.far_plane
        } else {
            1.0e7
        },
        vertical_fov: camera.vertical_fov,
        aspect_ratio: camera.aspect_ratio,
        depth_inverted: camera.depth_inverted,
        camera_motion_included: camera.camera_motion_included,
        reset: camera.reset,
        motion_invalid_value: if camera.has_motion_sentinel != 0 {
            camera.motion_sentinel
        } else {
            0.0
        },
        // The ABI uses null and zero for all optional hints and overrides.
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { out.write(frame) };
    0
}
