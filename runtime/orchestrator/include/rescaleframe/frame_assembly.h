/* SPDX-License-Identifier: GPL-3.0-only */
/* Compatibility conversion from decoded camera data and D3D11 textures to a DLSS frame.
   This legacy camera type is distinct from the SDK's rsf_camera_frame. Assembly copies metadata
   and resource pointers; it issues no GPU work and retains no references. */

#ifndef RSF_FRAME_ASSEMBLY_H
#define RSF_FRAME_ASSEMBLY_H

#include <rescaleframe/dlss.h>
#include <rescaleframe/runtime.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_FRAME_ASSEMBLY_ABI_VERSION 1u

typedef int32_t rsf_frame_assembly_result;
#define RSF_FRAME_ASSEMBLY_OK ((rsf_frame_assembly_result)0)
#define RSF_FRAME_ASSEMBLY_ERROR_INVALID_ARGUMENT ((rsf_frame_assembly_result)-1)
#define RSF_FRAME_ASSEMBLY_ERROR_ABI_MISMATCH ((rsf_frame_assembly_result)-2)
/* The frame describes a view that cannot drive a reconstruction: no jitter, no depth, or an output
   smaller than the render size. Refused here rather than producing a smeared image at runtime. */
#define RSF_FRAME_ASSEMBLY_ERROR_NOT_USABLE ((rsf_frame_assembly_result)-3)
/* Motion still in the game's own storage. A backend takes a scale factor and cannot subtract a
   bias, so the decode pass has to have run first. */
#define RSF_FRAME_ASSEMBLY_ERROR_MOTION_NOT_DECODED ((rsf_frame_assembly_result)-4)

/* One frame, as the plugin sees it. Engine terms, no vendor terms. */
typedef struct rsf_pipeline_camera_frame {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Row-major matrices with temporal jitter removed from the projection. */
    float view_to_clip[16];
    float clip_to_view[16];
    float clip_to_prev_clip[16];
    float prev_clip_to_clip[16];

    float camera_position[3];
    float camera_forward[3];
    float camera_up[3];
    float camera_right[3];

    /* Near/far distances and camera_position use the integration's view-space units. */
    float near_plane;
    /* Zero means an infinite far plane, which reversed-Z projections normally have. A backend
       wants a number, so one is chosen; see `rsf_assemble_dlss_frame`. */
    float far_plane;
    /* Radians. */
    float vertical_fov;
    float aspect_ratio;

    /* Sub-pixel offset in pixels at render resolution. Zero here is not "no jitter", it is a
       sample that happened to land at the centre, so `has_jitter` says which. */
    float jitter_pixels[2];
    uint32_t has_jitter;

    /* Whether depth closer to the camera holds the larger value. */
    uint32_t depth_inverted;
    /* Whether camera movement is folded into the motion buffer. Unreal writes object motion only,
       so this is normally false and a backend that can reconstruct the rest is told to. */
    uint32_t camera_motion_included;
    /* Whether a pass has decoded the motion out of the game's storage. Assembly refuses without
       it, because no backend can undo a bias. */
    uint32_t motion_decoded;
    /* What a decoded pixel holds where nothing wrote motion. Meaningless unless decoded. */
    uint32_t has_motion_sentinel;
    float motion_sentinel;
    /* Per-axis conversion from decoded values to the DLSS motion convention. The producer
       supplies the direction and basis; assembly copies these multipliers unchanged. */
    float motion_scale[2];

    /* No usable history: a cut, a teleport, or the first frame at a new resolution. */
    uint32_t reset;
} rsf_pipeline_camera_frame;

/* Borrowed ID3D11Texture2D inputs/output. Assembly checks pointer presence and declared extents,
   not texture descriptors, device ownership or resource contents. */
typedef struct rsf_frame_resources {
    uint32_t struct_size;
    void* color_in;
    void* color_out;
    void* depth;
    /* The decoded motion, not the game's own target. */
    void* motion;
    /* Optional. Without it a backend derives exposure itself, at some cost. */
    void* exposure;
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;
    rsf_dlss_quality quality;
} rsf_frame_resources;

/* Fill caller storage of at least sizeof(rsf_dlss_frame), preserving a nonzero out->struct_size.
   Refuses missing required resources, absent jitter, undecoded motion and output smaller than
   render size. A nonpositive far plane uses the legacy 1e7 view-unit fallback. Failure leaves
   out untouched; success does not establish image quality or resource lifetime. */
RSF_RUNTIME_API rsf_frame_assembly_result rsf_assemble_dlss_frame(
    const rsf_pipeline_camera_frame* camera, const rsf_frame_resources* resources, rsf_dlss_frame* out);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FRAME_ASSEMBLY_H */
