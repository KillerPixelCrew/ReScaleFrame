/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/depth_scale.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The bridge checks these stable DXGI values against the platform headers. */
#define RSF_AUX_FORMAT_RGBA32_FLOAT 2u
#define RSF_AUX_FORMAT_RGBA16_FLOAT 10u
#define RSF_AUX_FORMAT_R11G11B10_FLOAT 26u
#define RSF_AUX_FORMAT_RGBA8_UNORM 28u
#define RSF_AUX_FORMAT_RG16_FLOAT 34u
#define RSF_AUX_FORMAT_R32_FLOAT 41u
#define RSF_AUX_FORMAT_R24G8_TYPELESS 44u
#define RSF_AUX_FORMAT_R16_FLOAT 54u
#define RSF_AUX_FORMAT_R8_UNORM 61u

#define RSF_AUX_TEXTURE_BACKUP 0u
#define RSF_AUX_TEXTURE_SOURCE 1u
#define RSF_AUX_TEXTURE_OUTPUT 2u

typedef struct rsf_aux_texture_info {
    uint32_t width, height, format, sample_quality, depth_stencil;
} rsf_aux_texture_info;

typedef struct rsf_aux_draw_constants {
    float source[4], output[4], valid[4], jitter[2], padding[2];
} rsf_aux_draw_constants;

typedef struct rsf_aux_colour_pipeline {
    void *vertex, *pixel, *raster, *constants;
} rsf_aux_colour_pipeline;

/* Private callback table with every entry present. The Rust owner selects
   resources, operations and ordering. Successful COM outputs transfer one reference. */
typedef struct rsf_aux_platform {
    void* (*immediate_device)(void* context);
    int32_t (*device_ready)(void* device);
    int32_t (*texture_info)(void* texture, void* device, rsf_aux_texture_info*);
    int32_t (*supports_packed_colour)(void* device);
    int32_t (*create_pipeline)(void* device, rsf_game_log_fn, void* log_user, rsf_aux_colour_pipeline*);
    void* (*create_texture)(void* device, const rsf_aux_texture_info*, uint32_t usage);
    void* (*create_source_view)(void* device, void* texture);
    void* (*create_output_view)(void* device, void* texture);
    uint32_t (*save)(void* context, rsf_d3d11_state*);
    void (*restore)(void* context, rsf_d3d11_state*);
    void (*clear)(void* context);
    void (*copy)(void* context, void* destination, void* source);
    void (*draw)(void* context, const rsf_aux_colour_pipeline*, void* view, void* target, const rsf_aux_draw_constants*);
    int32_t (*depth_create)(void* context, const rsf_depth_scale_setup*, rsf_depth_scale**);
    int32_t (*depth_reserve)(rsf_depth_scale*, void* texture);
    int32_t (*depth_prepare)(rsf_depth_scale*, const rsf_depth_scale_frame*);
    int32_t (*depth_redraw)(rsf_depth_scale*, const rsf_depth_scale_frame*);
    int32_t (*depth_commit)(rsf_depth_scale*);
    int32_t (*depth_restore)(rsf_depth_scale*);
    int32_t (*depth_discard)(rsf_depth_scale*);
    int32_t (*depth_destroy)(rsf_depth_scale*);
} rsf_aux_platform;

#ifdef __cplusplus
}
#endif
