/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/motion_resolve.h>
#include "motion_d3d11_platform.h"

struct rsf_motion_resolve_resources {
    ID3D11ComputeShader* shader;
    ID3D11Buffer* constants;
    ID3D11Texture2D* textures[2];
    ID3D11UnorderedAccessView* views[2];
};

struct rsf_motion_resolve_platform {
    int32_t (*make)(void*, uint32_t width, uint32_t height, uint32_t bytes, rsf_motion_resolve_resources*);
    int32_t (*context)(void* device, void* context);
    int32_t (*describe)(void* device, void* source, rsf_motion_source_info*);
    void* (*source_view)(void* device, void* source, uint32_t format);
    int32_t (*dispatch)(void*, const rsf_motion_resolve_resources*, void* const* sources,
        const void* constants, uint32_t groups_x, uint32_t groups_y);
};

extern "C" {
int32_t rsf_core_motion_resolve_create(void*, uint32_t width, uint32_t height, rsf_motion_resolve**,
    const rsf_motion_resolve_platform*);
int32_t rsf_core_motion_resolve_run(rsf_motion_resolve*, void*, void*, void*, const rsf_motion_resolve_params*);
void* rsf_core_motion_resolve_texture(rsf_motion_resolve*, uint32_t output);
void rsf_core_motion_resolve_destroy(rsf_motion_resolve*);
}
