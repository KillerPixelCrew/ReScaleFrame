/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_fg.h>
#include <rescaleframe/native_window.h>

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
int32_t rsf_d3d11_host_fixture_recover(const rsf_game_host_services* host, void* context,
    void* chain, void* motion, const rsf_game_render_pass* base,
    rsf_d3d11_fixture_present_fn present,
    rsf_d3d11_fixture_disable_sr_fn disable_sr, void* user);

#define RSF_D3D11_FIXTURE_REDUCED_WARMUP 1u
#define RSF_D3D11_FIXTURE_LATE_ENABLE 2u
#define RSF_D3D11_FIXTURE_NATIVE_SR_OFF 3u
#define RSF_D3D11_FIXTURE_NATIVE_SR_ON 4u
#define RSF_D3D11_FIXTURE_FG_OFF 5u
/* Return nonzero to stop the phase. All leases are synchronous. */
typedef int32_t (*rsf_d3d11_fixture_frame_fn)(void* user, uint32_t reconstruct,
    uint32_t reset, int32_t expect_capture, uint32_t cleanup);
void rsf_d3d11_host_fixture_run_phase(uint32_t phase, uint32_t scoped,
    rsf_d3d11_fixture_frame_fn frame, void* user);

/* Synchronous fixture leases. Native code keeps ownership of the original
   application chain, fixture device, textures and views. */
typedef struct rsf_d3d11_fixture_resources {
    const rsf_game_host_services* host;
    void *device, *context, *chain;
    void *colour, *motion, *depth, *output;
    void *colour_view, *motion_view, *depth_view, *output_view;
    uint32_t width, height;
} rsf_d3d11_fixture_resources;

typedef struct rsf_d3d11_fixture_sr_calls {
    void* user;
    int32_t (*has_token)(void*, uint64_t source);
    int32_t (*set_quality)(void*, uint32_t quality);
    int32_t (*sr_status)(void*, int32_t* result, uint32_t* backend);
    int32_t (*cancel_dump)(uint64_t session);
    rsf_d3d11_fixture_present_fn present;
} rsf_d3d11_fixture_sr_calls;
int32_t rsf_d3d11_host_fixture_shared_sr(const rsf_d3d11_fixture_resources* resources,
    const rsf_d3d11_fixture_sr_calls* calls, const wchar_t* capture_root, rsf_game_render_pass* last);

typedef struct rsf_d3d11_fixture_fg_calls {
    void* user;
    /* On success, return the exact application chain's leased buffer with one reference. */
    int32_t (*get_buffer)(void* chain, void** texture);
    rsf_d3d11_fixture_present_fn present;
    int32_t (*has_provider_session)(void*);
    uint32_t (*backend)(void*);
    void (*outer_counts)(void*, uint32_t* pre, uint32_t* post, uint32_t* dispatched);
    int32_t (*pump_messages)(void);
} rsf_d3d11_fixture_fg_calls;

typedef struct rsf_d3d11_fixture_fg_state {
    uint64_t source, captured, sr_evaluated, app_presents;
    uint32_t failed, unavailable, occluded;
} rsf_d3d11_fixture_fg_state;

typedef struct rsf_d3d11_fixture_fg_frame {
    uint32_t reconstruct, primary, reset;
    int32_t expect_capture;
    uint32_t scoped, native_reinsertion, active_width, active_height;
    uint32_t native_pixel, outside_marker, cleanup, refuse_inputs_late, refuse_depth_late;
} rsf_d3d11_fixture_fg_frame;
void rsf_d3d11_host_fixture_rendered_frame(const rsf_d3d11_fixture_resources* resources,
    const rsf_d3d11_fixture_fg_calls* calls, const rsf_d3d11_fixture_fg_frame* frame,
    rsf_d3d11_fixture_fg_state* state);
#ifdef __cplusplus
}
#endif
