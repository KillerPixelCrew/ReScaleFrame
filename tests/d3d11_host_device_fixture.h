/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_window.h>
#include <rescaleframe/render_frontend.h>

#ifdef __cplusplus
extern "C" {
#endif
void* rsf_d3d11_host_fixture_texture(void* device, uint32_t width, uint32_t height,
    uint32_t format, uint32_t bindings);
int32_t rsf_d3d11_host_fixture_read_texture(void* device, void* context, void* source,
    uint32_t stride, unsigned char* bytes, size_t length);
int32_t rsf_d3d11_host_fixture_read_pixel(void* device, void* context, void* source,
    uint32_t x, uint32_t y, uint32_t stride, void* pixel);
void rsf_d3d11_host_fixture_cpu(const rsf_game_host_services* host, uint64_t source,
    uint32_t stage, uint32_t render_expected);

typedef int32_t (*rsf_d3d11_fixture_disable_sr_fn)(void* user);
typedef int32_t (*rsf_d3d11_fixture_present_fn)(void* chain);

/* Synchronous fixture leases. Native code keeps ownership of the original
   application chain, fixture device, textures and views. */
typedef struct rsf_d3d11_fixture_resources {
    const rsf_game_host_services* host;
    void *device, *context, *chain;
    void *colour, *motion, *depth, *output;
    void *colour_view, *motion_view, *depth_view, *output_view;
    uint32_t width, height;
    void* diagnostics;
    void (*close_window)(void);
} rsf_d3d11_fixture_resources;

typedef struct rsf_d3d11_fixture_sr_calls {
    void* user;
    int32_t (*has_token)(void*, uint64_t source);
    int32_t (*set_quality)(void*, uint32_t quality);
    int32_t (*sr_status)(void*, int32_t* result, uint32_t* backend);
    int32_t (*cancel_dump)(uint64_t session);
    rsf_d3d11_fixture_present_fn present;
    rsf_d3d11_fixture_disable_sr_fn disable_sr;
    float (*decode_half)(uint16_t);
} rsf_d3d11_fixture_sr_calls;
int32_t rsf_d3d11_host_fixture_shared_sr(const rsf_d3d11_fixture_resources* resources,
    const rsf_d3d11_fixture_sr_calls* calls, const wchar_t* capture_root);

typedef struct rsf_d3d11_fixture_fg_calls {
    void* user;
    /* On success, return the exact application chain's leased buffer with one reference. */
    int32_t (*get_buffer)(void* chain, void** texture);
    rsf_d3d11_fixture_present_fn present;
    int32_t (*has_provider_session)(void*);
    uint32_t (*backend)(void*);
    void (*outer_counts)(void*, uint32_t* pre, uint32_t* post, uint32_t* dispatched);
    int32_t (*pump_messages)(void);
    int32_t (*settings_get)(void*, rsf_render_settings*);
    int32_t (*settings_apply)(void*, const rsf_render_settings*);
    int32_t (*switch_result)(void*, int32_t* result);
    int32_t (*generation)(void*, uint64_t* generation);
} rsf_d3d11_fixture_fg_calls;

int32_t rsf_d3d11_host_fixture_rendered_fg(const rsf_d3d11_fixture_resources* resources,
    const rsf_d3d11_fixture_fg_calls* calls, uint32_t scoped, int32_t first_present,
    uint32_t native_pixel, uint32_t outside_marker);
int32_t rsf_d3d11_host_fixture_check_empty_aux(const rsf_game_host_services* host, void* context);
#ifdef __cplusplus
}
#endif
