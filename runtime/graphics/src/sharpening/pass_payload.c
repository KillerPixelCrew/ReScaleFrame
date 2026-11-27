/* SPDX-License-Identifier: GPL-3.0-only */
/* Native access to the existing shader sources and GPL constant mapping. */
#include "options.h"
#include "rcas.h"
#include "da_rcas.h"
#include "da_das.h"
#include "rcas_spirv.h"
#include "da_rcas_spirv.h"
#include "da_das_spirv.h"
#include "d3d11.h"
#include <stddef.h>
#include <string.h>

/* Marshal the separate MIT C settings record into the licensed calculator's inputs. */
static SharpenOptions options_from_request(const rsf_sharpening_options* o)
{
    SharpenOptions options = {o->mode, o->sharpness, o->mas, o->motion_sharpness,
        o->motion_threshold, o->motion_scale_limit, o->contrast_enabled, o->contrast,
        o->depth_scale, o->depth_bias, o->clamp_output, o->debug};
    return options;
}

const char* rsf_sharpening_hlsl(uint32_t mode)
{
    switch (mode) {
    case SHARPEN_MODE_RCAS: return rcas_hlsl;
    case SHARPEN_MODE_DEPTH_RCAS: return da_rcas_hlsl;
    case SHARPEN_MODE_DEPTH_DAS: return da_das_hlsl;
    default: return NULL;
    }
}

const uint32_t* rsf_sharpening_spirv(uint32_t mode, size_t* words)
{
    switch (mode) {
    case SHARPEN_MODE_RCAS: *words = sizeof(rcas_spirv) / sizeof(uint32_t); return rcas_spirv;
    case SHARPEN_MODE_DEPTH_RCAS:
        *words = sizeof(da_rcas_spirv) / sizeof(uint32_t);
        return da_rcas_spirv;
    case SHARPEN_MODE_DEPTH_DAS:
        *words = sizeof(da_das_spirv) / sizeof(uint32_t);
        return da_das_spirv;
    default: *words = 0; return NULL;
    }
}

uint32_t rsf_sharpen11_active(const rsf_sharpening_options* request)
{
    const SharpenOptions options = sharpen_sanitize(options_from_request(request));
    return sharpen_active(&options) ? 1u : 0u;
}

uint32_t rsf_sharpening_requested(const rsf_sharpening_options* request)
{
    const SharpenOptions options = options_from_request(request);
    return sharpen_active(&options) ? 1u : 0u;
}

void rsf_sharpen11_pack(const rsf_sharpening_options* request, const rsf_sharpen11_frame* facts,
    const uint32_t* extent, uint32_t exposure_texture, uint32_t mode, uint32_t* packed)
{
    const SharpenOptions options = options_from_request(request);
    SharpenFrame frame = {0};
    frame.depth_linear = facts->depth_linear;
    frame.depth_inverted = facts->depth_inverted;
    frame.hdr = facts->hdr;
    frame.near_plane = facts->near_plane;
    frame.far_plane = facts->far_plane;
    frame.exposure = facts->exposure;
    frame.motion_x = facts->motion_x;
    frame.motion_y = facts->motion_y;
    frame.width = extent[0];
    frame.height = extent[1];
    frame.motion_width = extent[2];
    frame.motion_height = extent[3];
    frame.depth_width = extent[4];
    frame.depth_height = extent[5];
    frame.exposure_texture = exposure_texture;
    const SharpenConstants data = sharpen_constants(&options, &frame, mode);
    memcpy(packed, &data, sizeof(data));
}

float rsf_sharpen11_adaptive(const rsf_sharpening_options* request, float base, float motion)
{
    const SharpenOptions options = options_from_request(request);
    return sharpen_adaptive(base, motion, &options, 1.0f);
}
