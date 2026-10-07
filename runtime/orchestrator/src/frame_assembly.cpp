// SPDX-License-Identifier: GPL-3.0-only

#include <rescaleframe/frame_assembly.h>

#include <cstring>

namespace {

// Legacy finite fallback for an infinite far plane: 1e7 view units, 100 km for AC7 centimetres.
// This is a chosen compatibility value, not a measured scene extent or universal world scale.
constexpr float default_far_plane = 1.0e7f;

} // namespace

extern "C" rsf_frame_assembly_result rsf_assemble_dlss_frame(const rsf_pipeline_camera_frame* camera,
                                                             const rsf_frame_resources* resources,
                                                             rsf_dlss_frame* out)
{
    if (!camera || !resources || !out || camera->struct_size < sizeof(rsf_pipeline_camera_frame) ||
        resources->struct_size < sizeof(rsf_frame_resources)) {
        return RSF_FRAME_ASSEMBLY_ERROR_INVALID_ARGUMENT;
    }
    if (camera->abi_version != RSF_FRAME_ASSEMBLY_ABI_VERSION) {
        return RSF_FRAME_ASSEMBLY_ERROR_ABI_MISMATCH;
    }
    if (!resources->color_in || !resources->color_out || !resources->depth || !resources->motion) {
        return RSF_FRAME_ASSEMBLY_ERROR_INVALID_ARGUMENT;
    }

    // Scale-only vendor motion parameters cannot subtract the packed engine encoding's bias.
    if (!camera->motion_decoded) {
        return RSF_FRAME_ASSEMBLY_ERROR_MOTION_NOT_DECODED;
    }

    // The reconstruction contract requires a jitter sequence, even on a zero-offset sample.
    if (!camera->has_jitter) {
        return RSF_FRAME_ASSEMBLY_ERROR_NOT_USABLE;
    }
    if (resources->render_width == 0 || resources->render_height == 0 ||
        resources->output_width < resources->render_width ||
        resources->output_height < resources->render_height) {
        return RSF_FRAME_ASSEMBLY_ERROR_NOT_USABLE;
    }

    const uint32_t saved_size = out->struct_size;
    std::memset(out, 0, sizeof(rsf_dlss_frame));
    out->struct_size = saved_size ? saved_size : uint32_t(sizeof(rsf_dlss_frame));
    out->abi_version = RSF_DLSS_ABI_VERSION;

    out->color_in = resources->color_in;
    out->color_out = resources->color_out;
    out->depth = resources->depth;
    out->motion = resources->motion;
    out->exposure = resources->exposure;

    out->render_width = resources->render_width;
    out->render_height = resources->render_height;
    out->output_width = resources->output_width;
    out->output_height = resources->output_height;
    out->quality = resources->quality;

    std::memcpy(out->camera_view_to_clip, camera->view_to_clip, sizeof(out->camera_view_to_clip));
    std::memcpy(out->clip_to_camera_view, camera->clip_to_view, sizeof(out->clip_to_camera_view));
    std::memcpy(out->clip_to_prev_clip, camera->clip_to_prev_clip, sizeof(out->clip_to_prev_clip));
    std::memcpy(out->prev_clip_to_clip, camera->prev_clip_to_clip, sizeof(out->prev_clip_to_clip));

    std::memcpy(out->camera_position, camera->camera_position, sizeof(out->camera_position));
    std::memcpy(out->camera_forward, camera->camera_forward, sizeof(out->camera_forward));
    std::memcpy(out->camera_up, camera->camera_up, sizeof(out->camera_up));
    std::memcpy(out->camera_right, camera->camera_right, sizeof(out->camera_right));

    out->jitter_x = camera->jitter_pixels[0];
    out->jitter_y = camera->jitter_pixels[1];
    out->motion_scale_x = camera->motion_scale[0];
    out->motion_scale_y = camera->motion_scale[1];

    out->near_plane = camera->near_plane;
    out->far_plane = camera->far_plane > 0.0f ? camera->far_plane : default_far_plane;
    out->vertical_fov = camera->vertical_fov;
    out->aspect_ratio = camera->aspect_ratio;

    out->depth_inverted = camera->depth_inverted;
    out->camera_motion_included = camera->camera_motion_included;
    out->reset = camera->reset;

    // An explicit unwritten marker lets the consumer reconstruct camera motion for sparse
    // object-only velocity. No sentinel leaves the decoded buffer's values unchanged.
    out->motion_invalid_value = camera->has_motion_sentinel ? camera->motion_sentinel : 0.0f;
    return RSF_FRAME_ASSEMBLY_OK;
}
