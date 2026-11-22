// SPDX-License-Identifier: MIT
// SDK descriptor, overlay ABI and CRT probes for the Rust fixture body.
#include <rescaleframe/render_frontend.h>
#include "../runtime/backends/common/sharpening.h"
#include <stdbool.h>
#include <wchar.h>
#include <rescaleframe/overlay.h>
#include <rescaleframe/d3d11_sr_api.h>
#include <stddef.h>
#ifdef RSF_SHARPENING_FFX_HEADER
#include <ffx_upscale.h>
#else
struct ffxDispatchDescUpscale { bool enableSharpening; float sharpness; };
#endif
#define RSF_ASSERT(condition) _Static_assert(condition, #condition)

RSF_ASSERT(RSF_RENDER_FRONTEND_ABI_VERSION == 3);
RSF_ASSERT(sizeof(rsf_render_settings) == 96);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_mode) == 48);
RSF_ASSERT(sizeof(rsf_sharpening_options) == 56);
RSF_ASSERT(RSF_OVERLAY_ABI_VERSION == 20);
RSF_ASSERT(sizeof(rsf_overlay_stats) == 440);
RSF_ASSERT(offsetof(rsf_overlay_stats, ui_preferences) == 340);
RSF_ASSERT(offsetof(rsf_overlay_stats, latency_paced_frames) == 352);
RSF_ASSERT(sizeof(rsf_overlay_intent) == 216);
RSF_ASSERT(offsetof(rsf_overlay_stats, saved_row_count) == 428);
RSF_ASSERT(offsetof(rsf_overlay_stats, saved_rows) == 432);
RSF_ASSERT(offsetof(rsf_overlay_intent, saved_action) == 204);
RSF_ASSERT(sizeof(rsf_overlay_saved_row) == 56);
RSF_ASSERT(offsetof(rsf_overlay_saved_row, text_length) == 16);
RSF_ASSERT(offsetof(rsf_overlay_stats, sr_quality_choices) == 416);
RSF_ASSERT(offsetof(rsf_overlay_stats, sr_change_status) == 424);
RSF_ASSERT(offsetof(rsf_overlay_intent, sr_change_id) == 200);
RSF_ASSERT(offsetof(rsf_overlay_intent, close_requested) == 188);
RSF_ASSERT(offsetof(rsf_overlay_intent, ui_preferences) == 196);
RSF_ASSERT(offsetof(rsf_overlay_stats, host_options) == 280);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_available) == 288);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_changed) == 136);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_mode) == 292);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_mode) == 140);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_mode) == 48);
RSF_ASSERT(offsetof(rsf_sharpening_options, mode) == 8);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_sharpness) == 296);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_sharpness) == 144);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_sharpness) == 52);
RSF_ASSERT(offsetof(rsf_sharpening_options, sharpness) == 12);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_mas) == 300);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_mas) == 148);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_mas) == 56);
RSF_ASSERT(offsetof(rsf_sharpening_options, mas) == 16);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_motion_sharpness) == 304);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_motion_sharpness) == 152);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_motion_sharpness) == 60);
RSF_ASSERT(offsetof(rsf_sharpening_options, motion_sharpness) == 20);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_motion_threshold) == 308);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_motion_threshold) == 156);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_motion_threshold) == 64);
RSF_ASSERT(offsetof(rsf_sharpening_options, motion_threshold) == 24);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_motion_scale_limit) == 312);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_motion_scale_limit) == 160);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_motion_scale_limit) == 68);
RSF_ASSERT(offsetof(rsf_sharpening_options, motion_scale_limit) == 28);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_contrast_enabled) == 316);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_contrast_enabled) == 164);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_contrast_enabled) == 72);
RSF_ASSERT(offsetof(rsf_sharpening_options, contrast_enabled) == 32);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_contrast) == 320);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_contrast) == 168);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_contrast) == 76);
RSF_ASSERT(offsetof(rsf_sharpening_options, contrast) == 36);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_depth_scale) == 324);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_depth_scale) == 172);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_depth_scale) == 80);
RSF_ASSERT(offsetof(rsf_sharpening_options, depth_scale) == 40);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_depth_bias) == 328);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_depth_bias) == 176);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_depth_bias) == 84);
RSF_ASSERT(offsetof(rsf_sharpening_options, depth_bias) == 44);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_clamp_output) == 332);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_clamp_output) == 180);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_clamp_output) == 88);
RSF_ASSERT(offsetof(rsf_sharpening_options, clamp_output) == 48);
RSF_ASSERT(offsetof(rsf_overlay_stats, sharpening_debug) == 336);
RSF_ASSERT(offsetof(rsf_overlay_intent, sharpening_debug) == 184);
RSF_ASSERT(offsetof(rsf_render_settings, sharpening_debug) == 92);
RSF_ASSERT(offsetof(rsf_sharpening_options, debug) == 52);
RSF_ASSERT(offsetof(rsf_d3d11_sr_api, request_dump) == 96);
RSF_ASSERT(offsetof(rsf_d3d11_sr_api, set_sharpening) == 104);

static int32_t set_sharpening(const rsf_sharpening_options* request)
{
    rsf_d3d11_sr_api table = {
        .struct_size = sizeof(table),
        .abi_version = RSF_D3D11_SR_API_VERSION,
    };
    if (rsf_get_d3d11_sr_api(&table) != 0 || !table.set_sharpening) return INT32_MIN;
    return table.set_sharpening(request);
}

static void ffx_fields(float request, uint32_t* enabled, float* amount)
{
    struct ffxDispatchDescUpscale dispatch = {0};
    dispatch.enableSharpening = true;
    dispatch.sharpness = 1;
    rsf_ffx_sharpening(request, &dispatch.enableSharpening, &dispatch.sharpness);
    *enabled = dispatch.enableSharpening;
    *amount = dispatch.sharpness;
}

static int32_t decimal(float amount, wchar_t* text, size_t size)
{
    return swprintf(text, size, L"%.9g", (double)amount);
}

typedef struct FixtureServices {
    int32_t (*set_sharpening)(const rsf_sharpening_options*);
    void (*ffx_fields)(float, uint32_t*, float*);
    int32_t (*decimal)(float, wchar_t*, size_t);
} FixtureServices;
RSF_ASSERT(sizeof(FixtureServices) == 3 * sizeof(void*));
#undef RSF_ASSERT

int32_t rsf_sharpening_settings_fixture(const FixtureServices*);

int main(void)
{
    const FixtureServices services = {set_sharpening, ffx_fields, decimal};
    return rsf_sharpening_settings_fixture(&services);
}
