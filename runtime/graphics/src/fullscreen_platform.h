/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/fullscreen_pass.h>
#include <rescaleframe/present_blit.h>
#include <d3d11.h>

struct rsf_fullscreen_resources {
    ID3D11VertexShader* vertex_shader;
    ID3D11PixelShader* pixel_shader;
    ID3D11SamplerState* point_sampler;
    ID3D11SamplerState* linear_sampler;
    ID3D11BlendState* opaque_blend;
    ID3D11BlendState* premultiplied_blend;
    ID3D11DepthStencilState* depth;
    ID3D11RasterizerState* raster;
    ID3D11Buffer* constants;
};

struct rsf_fullscreen_target_facts {
    uint32_t width, height, mip;
};

struct rsf_fullscreen_platform {
    int32_t (*make)(void* device, const rsf_fullscreen_setup*, uint32_t constant_bytes, rsf_fullscreen_resources*);
    int32_t (*immediate)(void* context);
    int32_t (*same_device)(void* device, void* child);
    int32_t (*target_facts)(void* target, rsf_fullscreen_target_facts*);
    int32_t (*draw)(void* context, void* target, void* source, const rsf_fullscreen_resources*,
        uint32_t width, uint32_t height, uint32_t linear, uint32_t blend, const void* constants, uint32_t bytes);
    void (*log)(rsf_fullscreen_log_fn, void* user, const char*);
};

const rsf_fullscreen_platform* rsf_fullscreen_native_platform();

struct rsf_present_blit_platform {
    void* (*back_buffer)(void* swapchain, uint32_t* width_height);
    void* (*target)(void* device, void* texture);
    void* (*source)(void* device, void* texture);
};

extern "C" {
int32_t rsf_core_fullscreen_create(void*, const rsf_fullscreen_setup*, rsf_fullscreen_pass**,
    const rsf_fullscreen_platform*);
int32_t rsf_core_fullscreen_draw(rsf_fullscreen_pass*, void*, void*, void*, const rsf_fullscreen_draw*);
void rsf_core_fullscreen_filter(rsf_fullscreen_pass*, uint32_t);
void rsf_core_fullscreen_destroy(rsf_fullscreen_pass*);
int32_t rsf_core_present_blit_create(void*, const rsf_present_blit_setup*, rsf_present_blit**,
    const rsf_fullscreen_platform*);
int32_t rsf_core_present_blit_draw(rsf_present_blit*, void*, void*, void*, uint32_t, const rsf_present_blit_platform*);
}
