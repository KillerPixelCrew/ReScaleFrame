/* SPDX-License-Identifier: MIT */
#pragma once
#include <stddef.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>

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
#ifdef __cplusplus
}
#endif
