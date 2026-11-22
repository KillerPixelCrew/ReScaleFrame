// SPDX-License-Identifier: MIT
#include <rescaleframe/native_cpu.h>
#include "api.h"
#include "godot_platform.h"
#include "vulkan_control.h"
#include <rescaleframe/input_dump_vulkan.h>
#include "../../../runtime/orchestrator/src/native_fg_fsr4_vulkan.h"

#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/native_streamline_vulkan.h>
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/fg_choice.h>
#include <windows.h>
#include <cstddef>
#include "../../../runtime/orchestrator/src/fg_auto_choice.h"

// CMake-selected provider symbols, C header helpers and the Rust platform table wiring.
static_assert(sizeof(rsf_cpu_session_service) == 32 && offsetof(rsf_cpu_session_service, execution) == 24);
static_assert(sizeof(rsf_sharpening_options) == 56);
static_assert(sizeof(rsf_godot_settings) == 100 && offsetof(rsf_godot_settings, sharpening) == 44);
static_assert(sizeof(rsf_godot_status) == 256 && offsetof(rsf_godot_status, sr_result) == 40 &&
    offsetof(rsf_godot_status, evaluated_frames) == 88 && offsetof(rsf_godot_status, presentation_statistics_valid) == 144 &&
    offsetof(rsf_godot_status, window_foreground) == 176 && offsetof(rsf_godot_status, keyboard_inputs) == 192 &&
    offsetof(rsf_godot_status, latency_owner) == 224 && offsetof(rsf_godot_status, derived_boundaries) == 248);
static_assert(sizeof(rsf_godot_setup) == 48 && sizeof(rsf_godot_engine_services) == 24);
static_assert(sizeof(rsf_godot_fg_options) == 20 && sizeof(rsf_godot_fg_install) == 96 &&
    offsetof(rsf_godot_fg_install, fidelityfx_directory) == 56);
static_assert(sizeof(rsf_godot_paths) == 56 && sizeof(rsf_godot_input_facts) == 56 && sizeof(rsf_godot_window_facts) == 16);
static_assert(sizeof(rsf_godot_fg_facts) == 88 && offsetof(rsf_godot_fg_facts, tagged_frames) == 40);
static_assert(sizeof(rsf_godot_latency_facts) == 96);
static_assert(sizeof(rsf_godot_sr_create) == 88 && offsetof(rsf_godot_sr_create, paths) == 32 &&
    sizeof(rsf_godot_sr_context) == 24 && offsetof(rsf_godot_sr_context, desired_width) == 12 &&
    sizeof(rsf_godot_sr_evaluate) == 64);
static_assert(sizeof(rsf_godot_option_field) == 16 && sizeof(rsf_godot_option_value) == 24);
static_assert(sizeof(rsf_godot_vk_facts) == 40 && sizeof(rsf_godot_vk_latency_facts) == 40 &&
    offsetof(rsf_godot_vk_latency_facts, sleep_calls) == 24);
static_assert(sizeof(rsf_godot_vk_sr_create) == 12 && sizeof(rsf_godot_vk_sr_context) == 16 &&
    offsetof(rsf_godot_vk_sr_context, result) == 8);
static_assert(offsetof(rsf_godot_platform, vulkan_locked) == 88 &&
    offsetof(rsf_godot_platform, latency_facts) == 176 && offsetof(rsf_godot_platform, d3d12_bind) == 184 &&
    offsetof(rsf_godot_platform, sr_evaluate) == 256 && offsetof(rsf_godot_platform, d3d12_release) == 320 &&
    offsetof(rsf_godot_platform, vulkan_presenter_drain) == 328 && offsetof(rsf_godot_platform, vulkan_sr_create) == 352 &&
    offsetof(rsf_godot_platform, vulkan_sr_record) == 368 && offsetof(rsf_godot_platform, vulkan_sr_dump_result) == 384 &&
    offsetof(rsf_godot_platform, window_identity) == 392 && offsetof(rsf_godot_platform, d3d12_cpu_service) == 432 &&
    offsetof(rsf_godot_platform, vulkan_present_marker) == 448 &&
    offsetof(rsf_godot_platform, sr_select_backend) == 456 && sizeof(rsf_godot_platform) == 472);
static_assert(sizeof(rsf_godot_window_identity) == 144 && offsetof(rsf_godot_window_identity, class_name) == 16);
static_assert(sizeof(rsf_game_cpu_event) == 64 && RSF_GAME_CPU_FRAME_BEGIN == 0 && RSF_GAME_CPU_INPUT_EVENT == 5 &&
    RSF_GAME_CPU_EVENT_VERSION == 1 && RSF_GAME_CPU_INPUT_KEYED == 1 && RSF_GAME_CPU_WINDOW_MESSAGE == 4,
    "native/window.rs builds window message events from rsf-game-sys");
static_assert(sizeof(WNDPROC) == sizeof(intptr_t) && sizeof(LRESULT) == sizeof(intptr_t) &&
    sizeof(WPARAM) == sizeof(uintptr_t) && sizeof(LPARAM) == sizeof(intptr_t) && sizeof(wchar_t) == sizeof(uint16_t));
static_assert(RSF_SR_AUTO == 7 && RSF_SR_SPATIAL == 6 && RSF_SR_DLSS == 1 && RSF_FG_BACKEND_AUTO == 7 &&
    RSF_FG_OFF == 0 && RSF_FG_FIXED == 1 && RSF_REFLEX_OFF == 0 && RSF_UI_MODE_BACKBUFFER_HUDLESS == 4 &&
    RSF_DLSS_ENGINE_CUSTOM == 0 && RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_BACKEND_OK == 0,
    "native/control.rs mirrors these Runtime constants");
static_assert(RSF_SR_NONE == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_SCREEN_UNKNOWN == 0 &&
    RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4 && RSF_GAME_RENDER_SR == 1 && RSF_GAME_RENDER_WINDOW == 9 &&
    RSF_GAME_RENDER_FINAL_SCENE == 10, "native/d3d12.rs mirrors these Runtime constants");

extern "C" {
void rsf_godot_host_unload();
uint64_t rsf_godot_host_tick_count();
uint32_t rsf_godot_host_prepare_capture();
int32_t rsf_godot_host_fg_install(const rsf_godot_fg_install* install);
void rsf_godot_host_fg_options(const rsf_godot_fg_options* value);
void rsf_godot_host_cpu_counters(rsf_godot_input_facts* facts);
void rsf_godot_host_window_facts(void* window, rsf_godot_window_facts* facts);
void rsf_godot_host_window_identity(void* window, rsf_godot_window_identity* identity);
intptr_t rsf_godot_host_window_procedure_get(void* window);
intptr_t rsf_godot_host_window_procedure_set(void* window, intptr_t procedure);
intptr_t rsf_godot_host_window_forward(intptr_t previous, void* window, uint32_t message, uintptr_t wparam, intptr_t lparam);
void rsf_godot_host_query_counter(uint64_t* counter, uint64_t* frequency);
void rsf_godot_host_fg_facts(rsf_godot_fg_facts* facts);
uint32_t rsf_godot_host_latency_facts(rsf_godot_latency_facts* facts);
uint32_t rsf_godot_host_d3d12_bind(void* owning_device, void* owning_queue);
uint32_t rsf_godot_host_d3d12_wait(uint64_t value);
uint32_t rsf_godot_host_d3d12_signal(uint64_t value);
uint32_t rsf_godot_host_sr_default_backend();
uint32_t rsf_godot_host_fg_default_backend();
uint32_t rsf_godot_host_present_owner(uint32_t* backend);
void rsf_godot_host_present_request(uint32_t backend);
uint32_t rsf_godot_host_sr_create(const rsf_godot_sr_create* request, rsf_godot_sr_context* created);
void rsf_godot_host_sr_destroy(void* context);
uint32_t rsf_godot_host_sr_evaluate(void* context, void* commands, const rsf_game_render_pass* pass,
    const rsf_godot_sr_evaluate* request, int32_t* result);
void rsf_godot_host_sr_refusal_log(const rsf_game_render_pass* pass, uint32_t desired_width, uint32_t desired_height);
uint32_t rsf_godot_host_capture_sr_begin(const rsf_game_render_pass* pass);
uint32_t rsf_godot_host_final_scene(void* commands, const rsf_game_render_pass* pass, uint32_t slot);
uint32_t rsf_godot_host_render_begin(uint64_t session, uint64_t frame);
uint32_t rsf_godot_host_fg_end_session(uint64_t session);
uint32_t rsf_godot_host_d3d12_release(void* context, uint64_t session);
}

extern "C" uint32_t rsf_godot_fg_preferred(uint32_t vendor)
{
    return rsf_fg_default_backend(vendor, false);
}

namespace {
struct HostLifetime {
    ~HostLifetime() { rsf_godot_host_unload(); }
};
const HostLifetime host_lifetime;

uint32_t leaf_dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
uint32_t leaf_sharpening_valid(const rsf_sharpening_options* options) { return rsf::sharpening_settings::valid(*options) ? 1u : 0u; }
void leaf_sharpening_defaults(rsf_sharpening_options* options) { *options = rsf_sharpening_defaults(); }
uint32_t leaf_fg_backend_known(uint32_t backend) { return rsf_fg_backend_known(backend) ? 1u : 0u; }
uint32_t leaf_fg_select_backend(uint32_t vendor, uint32_t software, uint32_t choices)
{
    return rsf_fg_select_auto_backend(rsf_fg_default_backend(vendor, software), choices);
}

const rsf_godot_platform platform{sizeof(platform), RSF_GODOT_PLATFORM_VERSION, rsf_godot_host_tick_count, rsf_godot_host_prepare_capture,
    rsf_godot_host_fg_install, rsf_godot_host_fg_options, leaf_dlss_preset_valid, leaf_sharpening_valid, leaf_sharpening_defaults,
    leaf_fg_backend_known, rsf_godot_vk_leaf_supported, rsf_godot_vk_leaf_latency_facts, rsf_godot_vk_leaf_locked,
    rsf_godot_vk_leaf_start, rsf_godot_vk_leaf_stop, rsf_godot_vk_leaf_release, rsf_godot_vk_leaf_facts,
    rsf_godot_vk_leaf_presenter_max, rsf_godot_vk_leaf_fsr4_fg_probe, rsf_godot_vk_leaf_latency_options,
    rsf_godot_host_cpu_counters, rsf_godot_host_window_facts, rsf_godot_host_fg_facts, rsf_godot_host_latency_facts,
    rsf_godot_host_d3d12_bind, rsf_godot_host_d3d12_wait, rsf_godot_host_d3d12_signal, rsf_godot_host_sr_default_backend, rsf_godot_host_fg_default_backend,
    rsf_godot_host_present_owner, rsf_godot_host_present_request, rsf_godot_host_sr_create, rsf_godot_host_sr_destroy, rsf_godot_host_sr_evaluate, rsf_godot_host_sr_refusal_log,
    rsf_godot_host_capture_sr_begin, rsf_godot_host_final_scene, rsf_fg12_submitted, rsf_godot_host_render_begin, rsf_fg12_window,
    rsf_godot_host_fg_end_session, rsf_godot_host_d3d12_release, rsf_godot_vk_leaf_presenter_drain, rsf_godot_vk_leaf_device_idle, rsf_godot_vk_leaf_sr_destroy,
    rsf_godot_vk_leaf_sr_create, rsf_godot_vk_leaf_sr_plan, rsf_godot_vk_leaf_sr_record, rsf_godot_vk_leaf_sr_log,
    rsf_godot_vk_leaf_sr_dump_result, rsf_godot_host_window_identity, rsf_godot_host_window_procedure_get, rsf_godot_host_window_procedure_set,
    rsf_godot_host_window_forward, rsf_godot_host_query_counter, rsf_fg12_cpu_service, rsf_latencyvk_cpu_service,
    rsf_latencyvk_present, rsf_gpu_select_sr_backend, leaf_fg_select_backend};
}

extern "C" const rsf_godot_platform* rsf_godot_platform_leaves(void) { return &platform; }

// CMake exposes the optional native provider symbols only in matching configurations.
struct rsf_godot_vk_features {
    uint32_t flags, reserved;
    decltype(&rsf_fsr4gvk_probe) fsr4_probe;
    decltype(&rsf_input_dump_vk_create) capture_create;
    decltype(&rsf_input_dump_vk_snapshot) capture_snapshot;
    decltype(&rsf_input_dump_vk_drain) capture_drain;
    decltype(&rsf_input_dump_vk_destroy) capture_destroy;
};
static_assert(sizeof(rsf_godot_vk_features) == 48 && offsetof(rsf_godot_vk_features, capture_create) == 16);

extern "C" const rsf_godot_vk_features* rsf_godot_vk_native_features() {
    static constexpr rsf_godot_vk_features features{
        0u
#if defined(RSF_VULKAN_SR_ENABLED)
        | 1u
#endif
#if defined(RSF_VULKAN_FSR4_ENABLED)
        | 2u
#endif
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
        | 4u
#endif
#if defined(RSF_VULKAN_XESS_FG_ENABLED)
        | 8u
#endif
        , 0,
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
        rsf_fsr4gvk_probe,
#else
        nullptr,
#endif
#if defined(RSF_FRAME_DUMP_VULKAN)
        rsf_input_dump_vk_create, rsf_input_dump_vk_snapshot, rsf_input_dump_vk_drain, rsf_input_dump_vk_destroy,
#else
        nullptr, nullptr, nullptr, nullptr,
#endif
    };
    return &features;
}
