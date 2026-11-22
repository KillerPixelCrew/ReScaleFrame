/* SPDX-License-Identifier: MIT */
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/native_window.h>

namespace {
void capture(void* context, const rsf_game_render_pass* pass, const rsf_d3d11_fg_inputs* inputs) noexcept
{
    if (!context || !pass || pass->struct_size < sizeof(*pass) ||
        !inputs || inputs->struct_size < sizeof(*inputs) ||
        inputs->abi_version != RSF_D3D11_FG_INPUTS_VERSION ||
        !inputs->depth || !inputs->motion || !pass->camera_valid) return;

    const auto width = [](const int32_t* rect) { return int64_t{rect[2]} - rect[0]; };
    const auto height = [](const int32_t* rect) { return int64_t{rect[3]} - rect[1]; };
    const auto render_width = width(pass->render_rect), render_height = height(pass->render_rect);
    const auto output_width = width(pass->output_rect), output_height = height(pass->output_rect);
    if (render_width <= 0 || render_height <= 0 || output_width <= 0 || output_height <= 0) return;

    // The existing shared transfer owner also receives normalized inputs from the SR pipeline.
    rsf_dlss_frame frame{};
    frame.struct_size = sizeof(frame);
    frame.abi_version = RSF_DLSS_ABI_VERSION;
    frame.depth = inputs->depth;
    frame.motion = inputs->motion;
    frame.render_width = static_cast<uint32_t>(render_width);
    frame.render_height = static_cast<uint32_t>(render_height);
    frame.output_width = static_cast<uint32_t>(output_width);
    frame.output_height = static_cast<uint32_t>(output_height);
    frame.camera_motion_included = 1;
    frame.reset = (pass->flags & RSF_GAME_RENDER_RESET) != 0;
    rsf_native_fg_capture_scoped(context, pass, &frame);
}

void present_event(void*, const rsf_observer_present_event* event) noexcept { rsf_native_window_present(event); }
void latency_event(void*, const rsf_observer_present_event* event) noexcept { rsf_native_fg_present(event); }
}

extern "C" RSF_RUNTIME_API rsf_result rsf_get_d3d11_fg_api(rsf_d3d11_fg_api* api)
{
    if (!api || api->struct_size < sizeof(*api)) return RSF_ERROR_INVALID_ARGUMENT;
    if (api->abi_version != RSF_D3D11_FG_API_VERSION) return RSF_ERROR_ABI_MISMATCH;
    const rsf_d3d11_fg_api complete{
        sizeof(rsf_d3d11_fg_api),
        RSF_D3D11_FG_API_VERSION,
        &rsf_native_fg_set_log,
        &rsf_native_fg_options_set,
        &rsf_native_fg_status_get,
        &rsf_native_fg_reflex_ordering_set,
        &rsf_native_fg_dispatch_cpu,
        &rsf_native_fg_dispatch_pass,
        &rsf_native_fg_bind_source,
        &capture,
        &rsf_native_fg_inputs_ready,
        &rsf_native_fg_dump_inputs,
        &rsf_native_fg_prepare,
        &rsf_native_fg_retire,
        &present_event,
        &latency_event,
    };
    *api = complete;
    return RSF_OK;
}
