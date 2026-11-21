/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/overlay_renderer.h>
#include <d3d11.h>
#include <stddef.h>

struct rsf_overlay11_pipeline {
    ID3D11VertexShader* vertex_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11InputLayout* input_layout;
    ID3D11Buffer* constants;
    ID3D11BlendState* blend;
    ID3D11DepthStencilState* depth_stencil;
    ID3D11RasterizerState* rasterizer;
    ID3D11SamplerState* sampler;
};

struct rsf_overlay11_draw {
    void* view;
    uint32_t index_count, index_offset;
    int32_t vertex_offset;
    int32_t clip[4];
};

struct rsf_overlay11_snapshot;

struct rsf_overlay11_platform {
    int32_t (*pipeline)(void*, const rsf_overlay_renderer_setup*, rsf_overlay11_pipeline*);
    int32_t (*context)(void* device, void* context);
    void* (*texture)(void* device, const rsf_overlay_texture_update*, uint32_t row_pitch);
    void* (*view)(void* device, void* texture);
    void (*patch)(void* context, void* texture, const rsf_overlay_texture_update*, uint32_t row_pitch);
    void* (*buffer)(void* device, uint32_t bytes, uint32_t bind_flags);
    int32_t (*write)(void* context, void* buffer, const void* bytes, size_t length);
    uint32_t (*target)(void* context);
    int32_t (*read_state)(void* context, rsf_overlay11_snapshot*);
    int32_t (*draw)(void* context, const rsf_overlay11_pipeline*, void* vertices, void* indices,
        uint32_t width, uint32_t height, const rsf_overlay11_draw*, size_t count);
    void (*restore_state)(void* context, const rsf_overlay11_snapshot*);
    void (*log)(rsf_overlay_renderer_log_fn, void* user, const char*);
};

extern "C" {
int32_t rsf_core_overlay11_create(void*, const rsf_overlay_renderer_setup*, rsf_overlay_renderer**,
    const rsf_overlay11_platform*);
int32_t rsf_core_overlay11_upload(rsf_overlay_renderer*, void*, const rsf_overlay_texture_update*);
void rsf_core_overlay11_free(rsf_overlay_renderer*, uint64_t id);
int32_t rsf_core_overlay11_draw(rsf_overlay_renderer*, void*, const rsf_overlay_draw_data*, uint32_t, uint32_t);
void rsf_core_overlay11_destroy(rsf_overlay_renderer*);
}
