// SPDX-License-Identifier: GPL-3.0-only
// Compatibility for the existing decoded-frame pipeline. New plugins submit SDK frame records.
#include "sr_legacy_adapter.h"
#include <rescaleframe/sr_bridge.h>
#include <rescaleframe/motion_resolve.h>
#include <windows.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <new>
#include <atomic>
namespace { std::atomic<uint64_t> next_session_id{1}; }
// Synthetic evaluation identity is private to SR continuity. It cannot authorize latency or FG;
// those paths require the native renderer's simulation/input-to-window association.
struct rsf_sr_legacy_adapter {
    rsf_sr_bridge* bridge = nullptr;
    rsf_motion_resolve* resolve = nullptr;
    void* device = nullptr;
    uint32_t width = 0, height = 0, generation = 0;
    uint64_t frame_id = 0;
    uint64_t session_id = next_session_id.fetch_add(1);
    LARGE_INTEGER previous{}, frequency{};
    float units_to_meters = 1;
};
rsf_backend_result rsf_sr_legacy_create(void* device, uint32_t width, uint32_t height,
    const char* fsr2, const char* fsr3, const char* fsr4, const char* xess,
    float units_to_meters, rsf_backend_log_fn log, void* user, rsf_sr_legacy_adapter** out)
{
    if (!out || !std::isfinite(units_to_meters) || units_to_meters <= 0) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = nullptr;
    auto* adapter = new (std::nothrow) rsf_sr_legacy_adapter;
    if (!adapter) return RSF_BACKEND_ERROR_INIT_FAILED;
    adapter->device = device; adapter->units_to_meters = units_to_meters;
    QueryPerformanceFrequency(&adapter->frequency); QueryPerformanceCounter(&adapter->previous);
    rsf_sr_session_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_SR_SESSION_ABI_VERSION;
    setup.open.struct_size = sizeof(setup.open); setup.open.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.open.api = RSF_API_D3D11; setup.open.device = device;
    setup.open.output_width = width; setup.open.output_height = height;
    setup.open.hdr = setup.open.inverted_depth = setup.open.dynamic_resolution = setup.open.auto_exposure = 1;
    setup.open.depth_infinite = 1;
    setup.open.log = log; setup.open.log_user = user;
    setup.fsr2_directory_utf8 = fsr2; setup.fsr3_directory_utf8 = fsr3; setup.fsr4_directory_utf8 = fsr4;
    setup.xess_directory_utf8 = xess;
    const auto result = rsf_sr_bridge_create(&setup, &adapter->bridge);
    if (result != 0) { delete adapter; return result; }
    *out = adapter; return RSF_BACKEND_OK;
}
rsf_backend_result rsf_sr_legacy_select(rsf_sr_legacy_adapter* adapter, uint32_t backend,
    rsf_quality quality, uint32_t* width, uint32_t* height)
{
    if (!adapter) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    const auto result = rsf_sr_bridge_select(adapter->bridge, backend, quality, 0);
    if (result == 0) {
        rsf_sr_session_status status{}; status.struct_size = sizeof(status);
        rsf_sr_bridge_get_status(adapter->bridge, &status);
        if (width) *width = status.render_width;
        if (height) *height = status.render_height;
    }
    return result;
}
rsf_backend_result rsf_sr_legacy_evaluate(rsf_sr_legacy_adapter* adapter, void* context,
    const rsf_dlss_frame* input, uint32_t has_sentinel)
{
    if (!adapter || !input) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    // FSR/XeSS choose exposure behavior at context creation, unlike DLSS's per-frame option.
    const auto exposure_policy = rsf_sr_bridge_set_auto_exposure(adapter->bridge, input->exposure ? 0u : 1u);
    if (exposure_policy != RSF_BACKEND_OK) return exposure_policy;
    if (adapter->width != input->render_width || adapter->height != input->render_height) {
        rsf_motion_resolve* replacement = nullptr;
        if (!rsf_motion_resolve_create(adapter->device, input->render_width, input->render_height, &replacement))
            return RSF_BACKEND_ERROR_INIT_FAILED;
        rsf_motion_resolve_destroy(adapter->resolve); adapter->resolve = replacement;
        adapter->width = input->render_width; adapter->height = input->render_height; ++adapter->generation;
    }
    rsf_motion_resolve_params resolve{};
    resolve.struct_size = sizeof(resolve);
    std::memcpy(resolve.clip_to_previous, input->clip_to_prev_clip, sizeof(resolve.clip_to_previous));
    // Compatibility assumption: written motion is current-minus-previous NDC. Convert to
    // previous-current pixels; each game's direction and camera coverage still need live validation.
    resolve.decoded_to_pixels[0] = -0.5f * input->render_width * input->motion_scale_x;
    resolve.decoded_to_pixels[1] = 0.5f * input->render_height * input->motion_scale_y;
    resolve.sentinel = input->motion_invalid_value; resolve.has_sentinel = has_sentinel;
    resolve.depth_layer = input->motion_depth_layer;
    if (!rsf_motion_resolve_run(adapter->resolve, context, input->motion, input->depth, &resolve))
        return RSF_BACKEND_ERROR_FEATURE_FAILED;
    // This interval measures calls to the compatibility adapter, not a game simulation delta.
    LARGE_INTEGER now{}; QueryPerformanceCounter(&now);
    rsf_frame_record record{};
    record.struct_size = sizeof(record); record.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.frame_id = ++adapter->frame_id; record.session_id = adapter->session_id; record.view_id = input->viewport;
    record.resource_generation = adapter->generation; record.flags = input->reset ? RSF_FRAME_FLAG_RESET : 0;
    record.render_width = input->render_width; record.render_height = input->render_height;
    record.output_width = input->output_width; record.output_height = input->output_height;
    record.frame_time_ms = std::clamp(float(double(now.QuadPart - adapter->previous.QuadPart) * 1000 /
                                          double(adapter->frequency.QuadPart)), 0.01f, 1000.0f);
    adapter->previous = now;
    record.camera.struct_size = sizeof(record.camera); record.camera.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    record.camera.near_plane = input->near_plane; record.camera.far_plane = input->far_plane;
    record.camera.vertical_fov_radians = input->vertical_fov; record.camera.depth_inverted = input->depth_inverted;
    rsf_sr_frame frame{}; frame.struct_size = sizeof(frame); frame.record = &record;
    frame.jitter_x = input->jitter_x; frame.jitter_y = input->jitter_y;
    frame.motion_scale_x = frame.motion_scale_y = 1; frame.pre_exposure = 1;
    frame.view_space_to_meters = adapter->units_to_meters; frame.reset = input->reset;
    void* textures[] = {input->color_in, rsf_motion_resolve_depth(adapter->resolve),
                        rsf_motion_resolve_motion(adapter->resolve), input->color_out};
    rsf_backend_resource* resources[] = {&frame.color, &frame.depth, &frame.motion, &frame.output};
    for (uint32_t i = 0; i < 4; ++i) {
        resources[i]->struct_size = sizeof(*resources[i]); resources[i]->resource = textures[i];
        resources[i]->width = i == 3 ? input->output_width : input->render_width;
        resources[i]->height = i == 3 ? input->output_height : input->render_height;
        resources[i]->generation = adapter->generation;
    }
    if (input->exposure) {
        frame.exposure.struct_size = sizeof(frame.exposure);
        frame.exposure.resource = input->exposure;
        frame.exposure.width = frame.exposure.height = 1;
        frame.exposure.generation = adapter->generation;
    }
    rsf_backend_resource* masks[] = {&frame.reactive, &frame.transparency};
    void* mask_textures[] = {input->reactive_mask, input->transparency_hint};
    for (uint32_t i = 0; i < 2; ++i) if (mask_textures[i]) {
        masks[i]->struct_size = sizeof(*masks[i]); masks[i]->resource = mask_textures[i];
        masks[i]->width = input->render_width; masks[i]->height = input->render_height;
        masks[i]->generation = adapter->generation;
    }
    return rsf_sr_bridge_evaluate(adapter->bridge, context, &frame);
}
void rsf_sr_legacy_destroy(rsf_sr_legacy_adapter* adapter)
{
    if (!adapter) return;
    rsf_sr_bridge_destroy(adapter->bridge); rsf_motion_resolve_destroy(adapter->resolve); delete adapter;
}
int rsf_sr_legacy_fg_inputs(rsf_sr_legacy_adapter* adapter, void** depth, void** motion)
{
    if (!adapter || !adapter->resolve || !depth || !motion) return 0;
    *depth = rsf_motion_resolve_depth(adapter->resolve);
    *motion = rsf_motion_resolve_motion(adapter->resolve);
    return *depth && *motion;
}
