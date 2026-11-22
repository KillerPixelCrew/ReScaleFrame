// SPDX-License-Identifier: MIT
// Presentation bridge, overlay, input dump and plugin session operations
// for the Rust shared callback render host in callback_host.rs. Each leaf performs one call.
// Admission, refusal codes, lifecycle order, host state, renderer configuration, slot and scene
// handling, settings and overlay control are decided in Rust. Returned COM interfaces carry one
// reference for the caller; arguments are borrowed for the call.
#include <rescaleframe/callback_render_host.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/sharpening_settings.h>
#include <cstddef>

// Keep native and generated size checks for both admitted setup versions.
static_assert(sizeof(rsf_callback_host_setup) == 128 && offsetof(rsf_callback_host_setup, probe) == 32 &&
              offsetof(rsf_callback_host_setup, backend) == 40 &&
              offsetof(rsf_callback_host_setup, engine_version_utf8) == 64 &&
              offsetof(rsf_callback_host_setup, xess_directory_utf8) == 112 &&
              offsetof(rsf_callback_host_setup, set_engine_spatial) == 120);
static_assert(sizeof(rsf_callback_host_setup_v2) == 160 && offsetof(rsf_callback_host_setup_v2, base) == 0);
static_assert(sizeof(rsf_plugin_session_options) == 128 && offsetof(rsf_plugin_session_options, services) == 24);
static_assert(RSF_CALLBACK_HOST_ABI_VERSION == 1 && RSF_PLUGIN_SESSION_ABI_VERSION == 1 && RSF_SR_DLSS == 1 &&
              RSF_SR_XESS == 5 && RSF_SR_SPATIAL == 6 && RSF_SR_AUTO == 7 &&
              RSF_BACKEND_ERROR_NOT_SUPPORTED == -7 && RSF_GAME_RENDER_RECONFIGURE == 0x80000000u);

namespace {
uint32_t depth_domains(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }
uint32_t dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
int32_t fg_choice_save(uint32_t backend) { return rsf_fg_choice_save(backend); }
void* present_host() { return rsf_d3d11_present_host(); }
int32_t present_request(uint32_t backend) { return rsf_d3d11_present_request(backend); }
uint32_t present_backend() { return rsf_d3d11_present_backend(); }
int32_t present_switch_result() { return rsf_d3d11_present_switch_result(); }
int32_t overlay_frame(void* device, void* queue, void* chain, const rsf_overlay_stats* stats, rsf_overlay_intent* intent,
    void (*log)(void*, const char*), void* user)
{
    return rsf_overlay_d3d12_frame(device, queue, chain, stats, intent, log, user);
}
void overlay_stop() { rsf_overlay_d3d12_stop(); }
void overlay_fill_fg(rsf_overlay_stats* stats, const rsf_native_fg_status* fg) { rsf_overlay_fill_fg_stats(stats, fg); }
void input_dump_cancel(uint64_t session) { rsf_input_dump_cancel(session); }
int32_t session_prepare(const rsf_plugin_session_options* options, void** out)
{
    return rsf_plugin_session_prepare(options, reinterpret_cast<rsf_plugin_session**>(out));
}
int32_t session_start(void* session) { return rsf_plugin_session_start(static_cast<rsf_plugin_session*>(session)); }
int32_t session_quiesce(void* session) { return rsf_plugin_session_quiesce(static_cast<rsf_plugin_session*>(session)); }
int32_t session_stop(void* session) { return rsf_plugin_session_stop(static_cast<rsf_plugin_session*>(session)); }

// Field order matches `Platform` in callback_host.rs.
struct Platform {
    uint32_t struct_size;
    decltype(&depth_domains) depth_domains;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&fg_choice_save) fg_choice_save;
    decltype(&present_host) present_host;
    decltype(&present_request) present_request;
    decltype(&present_backend) present_backend;
    decltype(&present_switch_result) present_switch_result;
    decltype(&overlay_frame) overlay_frame;
    decltype(&overlay_stop) overlay_stop;
    decltype(&overlay_fill_fg) overlay_fill_fg;
    decltype(&input_dump_cancel) input_dump_cancel;
    decltype(&session_prepare) session_prepare;
    decltype(&session_start) session_start;
    decltype(&session_quiesce) session_quiesce;
    decltype(&session_stop) session_stop;
};

constexpr Platform platform{
    sizeof(Platform),
    depth_domains, dlss_preset_valid,
    fg_choice_save,
    present_host, present_request, present_backend, present_switch_result, overlay_frame, overlay_stop,
    overlay_fill_fg, input_dump_cancel, session_prepare, session_start, session_quiesce, session_stop,
};
}

extern "C" const Platform* rsf_callback_host_platform()
{
    return &platform;
}
