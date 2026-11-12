/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/d3d11_fg_api.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/native_scene.h>
#include <rescaleframe/native_window.h>

namespace {
int cpu(const rsf_game_cpu_event* event) noexcept
{
    // The record comes first; latency and pacing act only on an accepted, ordered frame.
    if (!rsf_native_cpu_event(event)) return 0;
    rsf_native_fg_cpu(event);
    return 1;
}
// The same dispatch a statically linked carrier applies to its queued native passes.
int pass(void* context, const rsf_game_render_pass* pass, uint32_t begin) noexcept
{
    if (!pass || pass->struct_size < sizeof(rsf_game_render_pass)) return 0;
    switch (pass->role) {
    case RSF_GAME_RENDER_FRAME: rsf_native_fg_frame(pass, begin); return 1;
    case RSF_GAME_RENDER_SUBMISSION: rsf_native_fg_submission(pass, begin); return 1;
    case RSF_GAME_RENDER_FINAL_SCENE: {
        const int accepted = rsf_native_scene_pass(pass, begin);
        if (!begin && context) rsf_native_fg_final(context, pass);
        return accepted;
    }
    case RSF_GAME_RENDER_TEXTURE_BINDING: return begin ? 1 : rsf_native_scene_texture_binding(pass);
    case RSF_GAME_RENDER_WINDOW:
        if (!begin) rsf_native_fg_window_end(pass);
        return rsf_native_window_scope(pass, begin);
    default: return 1;
    }
}
void capture(void* context, const rsf_game_render_pass* identity, const rsf_dlss_frame* frame) noexcept
{
    if (!context || !identity || identity->struct_size < sizeof(rsf_game_render_pass) || !frame) return;
    rsf_native_fg_scene(identity);
    rsf_native_fg_capture(context, frame);
    rsf_native_fg_scene(nullptr);
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
        &cpu,
        &pass,
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
