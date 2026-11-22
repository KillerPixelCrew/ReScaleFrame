// SPDX-License-Identifier: MIT
// SDK descriptor, overlay ABI and CRT probes for the Rust fixture body.
#include <rescaleframe/render_frontend.h>
#include "../runtime/backends/common/sharpening.h"
#include <cwchar>
#include <rescaleframe/overlay.h>
#include <rescaleframe/d3d11_sr_api.h>
#include <cstddef>
#ifdef RSF_SHARPENING_FFX_HEADER
#include <ffx_upscale.h>
#else
struct ffxDispatchDescUpscale { bool enableSharpening; float sharpness; };
#endif
static_assert(RSF_RENDER_FRONTEND_ABI_VERSION == 3);
static_assert(sizeof(rsf_render_settings) == 96);
static_assert(offsetof(rsf_render_settings, sharpening_mode) == 48);
static_assert(sizeof(rsf_sharpening_options) == 56);
static_assert(RSF_OVERLAY_ABI_VERSION == 19);
static_assert(sizeof(rsf_overlay_stats) == 432);
static_assert(offsetof(rsf_overlay_stats, ui_preferences) == 340);
static_assert(offsetof(rsf_overlay_stats, latency_paced_frames) == 352);
static_assert(sizeof(rsf_overlay_intent) == 204);
static_assert(offsetof(rsf_overlay_stats, sr_quality_choices) == 416);
static_assert(offsetof(rsf_overlay_stats, sr_change_status) == 424);
static_assert(offsetof(rsf_overlay_intent, sr_change_id) == 200);
static_assert(offsetof(rsf_overlay_intent, close_requested) == 188);
static_assert(offsetof(rsf_overlay_intent, ui_preferences) == 196);
static_assert(offsetof(rsf_overlay_stats, host_options) == 280);
static_assert(offsetof(rsf_overlay_stats, sharpening_available) == 288);
static_assert(offsetof(rsf_overlay_intent, sharpening_changed) == 136);
static_assert(offsetof(rsf_overlay_stats, sharpening_mode) == 292);
static_assert(offsetof(rsf_overlay_intent, sharpening_mode) == 140);
static_assert(offsetof(rsf_render_settings, sharpening_mode) == 48);
static_assert(offsetof(rsf_sharpening_options, mode) == 8);
static_assert(offsetof(rsf_overlay_stats, sharpening_sharpness) == 296);
static_assert(offsetof(rsf_overlay_intent, sharpening_sharpness) == 144);
static_assert(offsetof(rsf_render_settings, sharpening_sharpness) == 52);
static_assert(offsetof(rsf_sharpening_options, sharpness) == 12);
static_assert(offsetof(rsf_overlay_stats, sharpening_mas) == 300);
static_assert(offsetof(rsf_overlay_intent, sharpening_mas) == 148);
static_assert(offsetof(rsf_render_settings, sharpening_mas) == 56);
static_assert(offsetof(rsf_sharpening_options, mas) == 16);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_sharpness) == 304);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_sharpness) == 152);
static_assert(offsetof(rsf_render_settings, sharpening_motion_sharpness) == 60);
static_assert(offsetof(rsf_sharpening_options, motion_sharpness) == 20);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_threshold) == 308);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_threshold) == 156);
static_assert(offsetof(rsf_render_settings, sharpening_motion_threshold) == 64);
static_assert(offsetof(rsf_sharpening_options, motion_threshold) == 24);
static_assert(offsetof(rsf_overlay_stats, sharpening_motion_scale_limit) == 312);
static_assert(offsetof(rsf_overlay_intent, sharpening_motion_scale_limit) == 160);
static_assert(offsetof(rsf_render_settings, sharpening_motion_scale_limit) == 68);
static_assert(offsetof(rsf_sharpening_options, motion_scale_limit) == 28);
static_assert(offsetof(rsf_overlay_stats, sharpening_contrast_enabled) == 316);
static_assert(offsetof(rsf_overlay_intent, sharpening_contrast_enabled) == 164);
static_assert(offsetof(rsf_render_settings, sharpening_contrast_enabled) == 72);
static_assert(offsetof(rsf_sharpening_options, contrast_enabled) == 32);
static_assert(offsetof(rsf_overlay_stats, sharpening_contrast) == 320);
static_assert(offsetof(rsf_overlay_intent, sharpening_contrast) == 168);
static_assert(offsetof(rsf_render_settings, sharpening_contrast) == 76);
static_assert(offsetof(rsf_sharpening_options, contrast) == 36);
static_assert(offsetof(rsf_overlay_stats, sharpening_depth_scale) == 324);
static_assert(offsetof(rsf_overlay_intent, sharpening_depth_scale) == 172);
static_assert(offsetof(rsf_render_settings, sharpening_depth_scale) == 80);
static_assert(offsetof(rsf_sharpening_options, depth_scale) == 40);
static_assert(offsetof(rsf_overlay_stats, sharpening_depth_bias) == 328);
static_assert(offsetof(rsf_overlay_intent, sharpening_depth_bias) == 176);
static_assert(offsetof(rsf_render_settings, sharpening_depth_bias) == 84);
static_assert(offsetof(rsf_sharpening_options, depth_bias) == 44);
static_assert(offsetof(rsf_overlay_stats, sharpening_clamp_output) == 332);
static_assert(offsetof(rsf_overlay_intent, sharpening_clamp_output) == 180);
static_assert(offsetof(rsf_render_settings, sharpening_clamp_output) == 88);
static_assert(offsetof(rsf_sharpening_options, clamp_output) == 48);
static_assert(offsetof(rsf_overlay_stats, sharpening_debug) == 336);
static_assert(offsetof(rsf_overlay_intent, sharpening_debug) == 184);
static_assert(offsetof(rsf_render_settings, sharpening_debug) == 92);
static_assert(offsetof(rsf_sharpening_options, debug) == 52);
static_assert(offsetof(rsf_d3d11_sr_api, request_dump) == 96);
static_assert(offsetof(rsf_d3d11_sr_api, set_sharpening) == 104);

namespace {
int32_t set_sharpening(const rsf_sharpening_options* request)
{
    rsf_d3d11_sr_api table{sizeof(table), RSF_D3D11_SR_API_VERSION};
    if (rsf_get_d3d11_sr_api(&table) != 0 || !table.set_sharpening) return INT32_MIN;
    return table.set_sharpening(request);
}

void ffx_fields(float request, uint32_t* enabled, float* amount)
{
    ffxDispatchDescUpscale dispatch{};
    dispatch.enableSharpening = true;
    dispatch.sharpness = 1;
    rsf_ffx_sharpening(request, &dispatch.enableSharpening, &dispatch.sharpness);
    *enabled = dispatch.enableSharpening;
    *amount = dispatch.sharpness;
}

int32_t decimal(float amount, wchar_t* text, size_t size)
{
    return std::swprintf(text, size, L"%.9g", static_cast<double>(amount));
}
}

struct FixtureServices {
    decltype(&::set_sharpening) set_sharpening;
    decltype(&::ffx_fields) ffx_fields;
    decltype(&::decimal) decimal;
};
static_assert(sizeof(FixtureServices) == 3 * sizeof(void*));
extern "C" int32_t rsf_sharpening_settings_fixture(const FixtureServices*);

int main()
{
    const FixtureServices services{set_sharpening, ffx_fields, decimal};
    return rsf_sharpening_settings_fixture(&services);
}
