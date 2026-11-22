/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <rescaleframe/d3d11_state.h>

#ifdef __cplusplus
extern "C" {
#endif

/* D3D11 texture description and format codes the Rust render paths share; the leaves that use them
   are Rust (rsf_graphics::d3d11_output_platform). */
#define RSF_D3D11_FORMAT_RGBA32_FLOAT 2u
#define RSF_D3D11_FORMAT_RGBA16_FLOAT 10u
#define RSF_D3D11_FORMAT_RG32_FLOAT 16u
#define RSF_D3D11_FORMAT_R11G11B10_FLOAT 26u
#define RSF_D3D11_FORMAT_RG16_FLOAT 34u
#define RSF_D3D11_FORMAT_R32_FLOAT 41u
#define RSF_D3D11_USAGE_DEFAULT 0u
#define RSF_D3D11_USAGE_IMMUTABLE 1u
#define RSF_D3D11_BIND_SHADER_RESOURCE 8u
#define RSF_D3D11_BIND_RENDER_TARGET 32u

typedef struct rsf_d3d11_texture_desc {
    uint32_t width, height, mips, layers, format, samples, sample_quality;
    uint32_t usage, bind_flags, cpu_access_flags, misc_flags;
} rsf_d3d11_texture_desc;

struct rsf_fullscreen_pass;

#ifdef __cplusplus
}
#endif
