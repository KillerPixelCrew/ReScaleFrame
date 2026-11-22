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

struct rsf_overlay11_platform {
    int32_t (*pipeline)(void*, const rsf_overlay_renderer_setup*, rsf_overlay11_pipeline*);
    void (*log)(rsf_overlay_renderer_log_fn, void* user, const char*);
};

extern "C" {
int32_t rsf_core_overlay11_create(void*, const rsf_overlay_renderer_setup*, rsf_overlay_renderer**,
    const rsf_overlay11_platform*);
}
