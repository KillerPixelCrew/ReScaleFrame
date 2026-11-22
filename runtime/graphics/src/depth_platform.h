/* SPDX-License-Identifier: MIT */
#pragma once
/* Native D3D11 leaves for the Rust depth owners. Rust owns the transactions, acceptance, leases
   and ordering; these only issue typed device and context calls. */
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/depth_replay.h>
#include <rescaleframe/depth_scale.h>
#include <d3d11.h>

struct rsf_depth_scale_pipeline {
    ID3D11VertexShader* vertex;
    ID3D11PixelShader* depth_shader;
    ID3D11PixelShader* stencil_shader;
    ID3D11DepthStencilState* depth_state;
    ID3D11DepthStencilState* stencil_states[8];
    ID3D11RasterizerState* raster;
    ID3D11Buffer* constants;
};

struct rsf_depth_scale_surfaces {
    ID3D11Texture2D* snapshot;
    ID3D11Texture2D* output;
    ID3D11ShaderResourceView* depth_view;
    ID3D11ShaderResourceView* stencil_view;
    ID3D11DepthStencilView* output_view;
};

struct rsf_depth_scale_platform {
    /* Retained device of an immediate context, or null. */
    void* (*device)(void* context);
    int32_t (*make)(void* device, const rsf_depth_scale_setup*, uint32_t constant_bytes,
        rsf_depth_scale_pipeline*);
    /* Owner and depth layout of a candidate texture; writes width and height on success. */
    int32_t (*describe)(void* device, void* texture, uint32_t* width_height);
    int32_t (*ready)(void* device);
    /* Snapshot, output and their views shaped like texture. All or nothing. */
    int32_t (*surfaces)(void* device, void* texture, rsf_depth_scale_surfaces*);
    uint32_t (*save)(void* context, rsf_d3d11_state*);
    void (*restore)(void* context, rsf_d3d11_state*);
    void (*clear)(void* context);
    void (*copy)(void* context, void* destination, void* source);
    void (*bind)(void* context, const rsf_depth_scale_pipeline*, const rsf_depth_scale_surfaces*,
        const int32_t* output_rect);
    /* Step 0 writes depth; step n writes stencil bit n - 1. */
    void (*draw)(void* context, const rsf_depth_scale_pipeline*, const void* constants, uint32_t step);
};

extern const rsf_depth_scale_platform rsf_depth_scale_native_platform;

struct rsf_depth_replay_resources {
    ID3D11Texture2D* texture;
    ID3D11DepthStencilView* view;
    ID3D11DepthStencilState* write;
};

/* The part of a tap geometry report the Rust owner judges. The report itself stays borrowed
   for the native draw. */
struct rsf_depth_replay_candidate {
    void* context;
    void* target;
    void* depth_view;
    uint32_t width, height, samples, kind, count, instances, topology, vertex_shader;
    const rsf_frame_tap_geometry* geometry;
};

struct rsf_depth_view_info {
    uint32_t format, dimension, flags, mip_slice;
};

struct rsf_depth_texture_info {
    uint32_t width, height, format, samples, quality, mips, array;
};

struct rsf_depth_pipeline_info {
    uint32_t stages, stream_out, uavs, depth_state;
    uint32_t depth_enable, depth_write_mask, depth_func, stencil_enable, front_func, back_func;
    uint32_t viewport_count;
    float viewport[6];
};

struct rsf_depth_replay_platform {
    int32_t (*make)(void* device, uint32_t width, uint32_t height, rsf_depth_replay_resources*);
    void (*view)(void* depth_view, rsf_depth_view_info*);
    /* Retained 2D texture behind the view, or null. */
    void* (*texture)(void* depth_view, rsf_depth_texture_info*);
    void (*pipeline)(void* context, rsf_depth_pipeline_info*);
    uint32_t (*save)(void* context, rsf_d3d11_state*);
    void (*restore)(void* context, rsf_d3d11_state*);
    void (*unbind)(void* context);
    void (*copy)(void* context, void* destination, void* source);
    void (*draw)(void* context, const rsf_depth_replay_resources*, const rsf_frame_tap_geometry*);
};

extern "C" {
int32_t rsf_core_depth_scale_create(void*, const rsf_depth_scale_setup*, rsf_depth_scale**,
    const rsf_depth_scale_platform*);
uint32_t rsf_core_depth_scale_reserve(rsf_depth_scale*, void* texture);
int32_t rsf_core_depth_scale_render(rsf_depth_scale*, const rsf_depth_scale_frame*, uint32_t capture);
int32_t rsf_core_depth_scale_commit(rsf_depth_scale*);
int32_t rsf_core_depth_scale_restore(rsf_depth_scale*);
int32_t rsf_core_depth_scale_discard(rsf_depth_scale*);
int32_t rsf_core_depth_scale_destroy(rsf_depth_scale*);

rsf_depth_replay* rsf_core_depth_replay_create(void* device, uint32_t width, uint32_t height,
    const rsf_depth_replay_platform*);
uint32_t rsf_core_depth_replay_last_reject(const rsf_depth_replay*);
void rsf_core_depth_replay_get_detail(const rsf_depth_replay*, rsf_depth_replay_detail*);
uint32_t rsf_core_depth_replay_draw(rsf_depth_replay*, const rsf_depth_replay_candidate*);
void* rsf_core_depth_replay_selected(rsf_depth_replay*, void* context, void* depth, void* layer);
void rsf_core_depth_replay_end_frame(rsf_depth_replay*);
void rsf_core_depth_replay_destroy(rsf_depth_replay*);
}
