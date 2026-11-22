// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>
#ifndef __cplusplus
#include <stdbool.h>
#endif

typedef struct rsf_anomaly_motion_fixture_input {
    void* layout;
    void* vertex_buffer;
    void* vs;
    void* hs;
    void* ds;
    void* ps;
    uint32_t stride;
    bool tessellated;
} rsf_anomaly_motion_fixture_input;

typedef struct rsf_anomaly_motion_fixture_output {
    void* position;
    void* albedo;
    void* motion;
    void* depth;
    void* depth_state;
    void* raster;
    uint32_t size;
} rsf_anomaly_motion_fixture_output;

typedef struct rsf_anomaly_motion_fixture_override {
    const char* name;
    const float* values;
    size_t count;
} rsf_anomaly_motion_fixture_override;

typedef struct rsf_anomaly_motion_fixture_targets {
    void* position_texture;
    void* position_rtv;
    void* albedo_texture;
    void* albedo_rtv;
    void* depth_texture;
    void* depth_view;
    void* depth_state;
    void* raster;
} rsf_anomaly_motion_fixture_targets;

typedef struct rsf_anomaly_motion_fixture_bindings rsf_anomaly_motion_fixture_bindings;

#ifdef __cplusplus
extern "C" {
#endif
bool rsf_anomaly_motion_fixture_read(void* device, void* context, void* texture, void* out, size_t bytes);
void rsf_anomaly_motion_fixture_clear(void* context, const rsf_anomaly_motion_fixture_output*, bool stencil);
void rsf_anomaly_motion_fixture_input_bind(void* context, const rsf_anomaly_motion_fixture_input*);
void rsf_anomaly_motion_fixture_output_bind(void* context, const rsf_anomaly_motion_fixture_output*);
void rsf_anomaly_motion_fixture_draw(void* context, uint32_t count);
void rsf_anomaly_motion_fixture_draw_quad(void* context, void* buffer, uint32_t stride);
// All eight COM references transfer together on success. A refusal leaves the output untouched.
const char* rsf_anomaly_motion_fixture_make_targets(void* device, uint32_t size, bool stencil,
    rsf_anomaly_motion_fixture_targets*);
// Lends D3D11_SIGNATURE_PARAMETER_DESC rows and the stock bytecode. Returns one owned input-layout reference.
const char* rsf_anomaly_motion_fixture_make_layout(void* device, const void* signature, uint32_t count,
    const void* stock, size_t stock_size, void** layout);
rsf_anomaly_motion_fixture_bindings* rsf_anomaly_motion_fixture_bindings_new(void);
void rsf_anomaly_motion_fixture_bindings_drop(rsf_anomaly_motion_fixture_bindings*);
const char* rsf_anomaly_motion_fixture_bind_stage(rsf_anomaly_motion_fixture_bindings*, void* device, void* context,
    uint8_t stage, bool tessellated, const void* code, size_t code_size,
    const rsf_anomaly_motion_fixture_override*, size_t override_count);
const char* rsf_anomaly_motion_fixture_bind_core(rsf_anomaly_motion_fixture_bindings*, void* device, void* context,
    void* reflection, bool vertex);
const char* rsf_anomaly_motion_fixture_patch_core(rsf_anomaly_motion_fixture_bindings*, void* device, void* context,
    void* reflection, const rsf_anomaly_motion_fixture_override*, size_t override_count);
#ifdef __cplusplus
}
#endif
