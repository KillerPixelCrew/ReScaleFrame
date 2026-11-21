/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/motion_decode.h>
#include "motion_d3d11_platform.h"

struct rsf_motion_decode_resources {
    ID3D11ComputeShader* shader;
    ID3D11Texture2D* target;
    ID3D11UnorderedAccessView* target_view;
    ID3D11Buffer* constants;
};

struct rsf_motion_decode_platform {
    int32_t (*make)(void* device, const rsf_motion_decode_setup*, uint32_t constant_bytes,
        rsf_motion_decode_resources*);
    int32_t (*context)(void* device, void* context);
    int32_t (*describe)(void* device, void* source, rsf_motion_source_info*);
    void* (*source_view)(void* device, void* source, uint32_t format);
    int32_t (*dispatch)(void* context, const rsf_motion_decode_resources*, void* source_view,
        const void* constants, size_t bytes, uint32_t groups_x, uint32_t groups_y);
    void (*log)(rsf_motion_decode_log_fn, void*, const char*);
};

extern "C" {
int32_t rsf_core_motion_decode_create(void*, const rsf_motion_decode_setup*, rsf_motion_decode**,
    const rsf_motion_decode_platform*);
int32_t rsf_core_motion_decode_run(rsf_motion_decode*, void*, void*, const rsf_motion_decode_params*);
void* rsf_core_motion_decode_texture(rsf_motion_decode*);
void rsf_core_motion_decode_destroy(rsf_motion_decode*);
}
