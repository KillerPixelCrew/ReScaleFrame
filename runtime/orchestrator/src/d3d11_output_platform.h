/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>
#include <rescaleframe/d3d11_state.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private C bridge. Each non-null object returned by a query or creation call
   carries one COM reference. */
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

void* rsf_d3d11_output_query_texture(void* resource);
void* rsf_d3d11_output_child_device(void* child);
void rsf_d3d11_output_texture_desc(void* texture, rsf_d3d11_texture_desc* desc);
void* rsf_d3d11_output_create_texture(void* device, const rsf_d3d11_texture_desc* desc);
void* rsf_d3d11_output_create_srv(void* device, void* texture, uint32_t format);
void* rsf_d3d11_output_create_rtv(void* device, void* texture);
uint32_t rsf_d3d11_output_srv_format(uint32_t format);
uint32_t rsf_d3d11_output_device_ok(void* device);
uint32_t rsf_d3d11_output_is_immediate(void* context);
void rsf_d3d11_output_clear_state(void* context);
void rsf_d3d11_output_unbind_targets(void* context);
void rsf_d3d11_output_copy_resource(void* context, void* destination, void* source);
void rsf_d3d11_output_copy_region(void* context, void* destination, void* source,
    uint32_t left, uint32_t top, uint32_t width, uint32_t height);

struct rsf_fullscreen_pass;

#ifdef __cplusplus
}
#endif
