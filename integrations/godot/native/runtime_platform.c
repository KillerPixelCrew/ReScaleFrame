/* SPDX-License-Identifier: MIT */
/* The native side of ReScaleFrame.Godot.Native.dll that Rust cannot express: layout checks of the
   private records against the C headers, the CMake-selected optional Vulkan provider symbols, the
   CMake project version and the DLL unload hook. The GDExtension classes, every rsfg_* export and
   all decisions are Rust (native/lib.rs). */
#include <rescaleframe/native_cpu.h>
#include "api.h"
#include "godot_platform.h"
#include "vulkan_features.h"

#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/version.h>
#include <windows.h>
#include <stddef.h>
#include <stdlib.h>

/* The private extension records, as native/platform.rs mirrors them. */
_Static_assert(sizeof(rsf_cpu_session_service) == 32 && offsetof(rsf_cpu_session_service, execution) == 24,
    "rsf_cpu_session_service");
_Static_assert(sizeof(rsf_sharpening_options) == 56, "rsf_sharpening_options");
_Static_assert(sizeof(rsf_godot_settings) == 100 && offsetof(rsf_godot_settings, sharpening) == 44,
    "rsf_godot_settings");
_Static_assert(sizeof(rsf_godot_status) == 256 && offsetof(rsf_godot_status, sr_result) == 40 &&
    offsetof(rsf_godot_status, evaluated_frames) == 88 && offsetof(rsf_godot_status, presentation_statistics_valid) == 144 &&
    offsetof(rsf_godot_status, window_foreground) == 176 && offsetof(rsf_godot_status, keyboard_inputs) == 192 &&
    offsetof(rsf_godot_status, latency_owner) == 224 && offsetof(rsf_godot_status, derived_boundaries) == 248,
    "rsf_godot_status");
_Static_assert(sizeof(rsf_godot_setup) == 48 && sizeof(rsf_godot_engine_services) == 24, "rsf_godot_setup");
_Static_assert(sizeof(rsf_godot_fg_options) == 20 && sizeof(rsf_godot_fg_install) == 96 &&
    offsetof(rsf_godot_fg_install, fidelityfx_directory) == 56, "rsf_godot_fg_install");
_Static_assert(sizeof(rsf_godot_paths) == 56 && sizeof(rsf_godot_input_facts) == 56 &&
    sizeof(rsf_godot_window_facts) == 16, "rsf_godot_paths");
_Static_assert(sizeof(rsf_godot_fg_facts) == 88 && offsetof(rsf_godot_fg_facts, tagged_frames) == 40,
    "rsf_godot_fg_facts");
_Static_assert(sizeof(rsf_godot_latency_facts) == 96, "rsf_godot_latency_facts");
_Static_assert(sizeof(rsf_godot_sr_create) == 88 && offsetof(rsf_godot_sr_create, paths) == 32 &&
    sizeof(rsf_godot_sr_context) == 24 && offsetof(rsf_godot_sr_context, desired_width) == 12 &&
    sizeof(rsf_godot_sr_evaluate) == 64, "rsf_godot_sr_create");
_Static_assert(sizeof(rsf_godot_option_field) == 16 && sizeof(rsf_godot_option_value) == 24,
    "rsf_godot_option_field");
_Static_assert(sizeof(rsf_godot_vk_facts) == 40 && sizeof(rsf_godot_vk_latency_facts) == 40 &&
    offsetof(rsf_godot_vk_latency_facts, sleep_calls) == 24, "rsf_godot_vk_facts");
_Static_assert(sizeof(rsf_godot_vk_sr_create) == 12 && sizeof(rsf_godot_vk_sr_context) == 16 &&
    offsetof(rsf_godot_vk_sr_context, result) == 8, "rsf_godot_vk_sr_context");
_Static_assert(offsetof(rsf_godot_platform, vulkan_locked) == 88 &&
    offsetof(rsf_godot_platform, latency_facts) == 176 && offsetof(rsf_godot_platform, d3d12_bind) == 184 &&
    offsetof(rsf_godot_platform, sr_evaluate) == 256 && offsetof(rsf_godot_platform, d3d12_release) == 320 &&
    offsetof(rsf_godot_platform, vulkan_presenter_drain) == 328 && offsetof(rsf_godot_platform, vulkan_sr_create) == 352 &&
    offsetof(rsf_godot_platform, vulkan_sr_record) == 368 && offsetof(rsf_godot_platform, vulkan_sr_dump_result) == 384 &&
    offsetof(rsf_godot_platform, window_identity) == 392 && offsetof(rsf_godot_platform, d3d12_cpu_service) == 432 &&
    offsetof(rsf_godot_platform, vulkan_present_marker) == 448 &&
    offsetof(rsf_godot_platform, sr_select_backend) == 456 && sizeof(rsf_godot_platform) == 472,
    "rsf_godot_platform");
_Static_assert(sizeof(rsf_godot_window_identity) == 144 && offsetof(rsf_godot_window_identity, class_name) == 16,
    "rsf_godot_window_identity");
_Static_assert(sizeof(rsf_game_cpu_event) == 64 && RSF_GAME_CPU_FRAME_BEGIN == 0 && RSF_GAME_CPU_INPUT_EVENT == 5 &&
    RSF_GAME_CPU_EVENT_VERSION == 1 && RSF_GAME_CPU_INPUT_KEYED == 1 && RSF_GAME_CPU_WINDOW_MESSAGE == 4,
    "native/window.rs builds window message events from rsf-game-sys");
_Static_assert(sizeof(WNDPROC) == sizeof(intptr_t) && sizeof(LRESULT) == sizeof(intptr_t) &&
    sizeof(WPARAM) == sizeof(uintptr_t) && sizeof(LPARAM) == sizeof(intptr_t) && sizeof(wchar_t) == sizeof(uint16_t),
    "Win32 handle and character widths");
_Static_assert(RSF_SR_AUTO == 7 && RSF_SR_SPATIAL == 6 && RSF_SR_DLSS == 1 && RSF_FG_BACKEND_AUTO == 7 &&
    RSF_FG_OFF == 0 && RSF_FG_FIXED == 1 && RSF_REFLEX_OFF == 0 && RSF_UI_MODE_BACKBUFFER_HUDLESS == 4 &&
    RSF_DLSS_ENGINE_CUSTOM == 0 && RSF_FG_STAT_TOTAL_PRESENTED == 1 && RSF_BACKEND_OK == 0,
    "native/control.rs mirrors these Runtime constants");
_Static_assert(RSF_SR_NONE == 0 && RSF_BACKEND_ERROR_INVALID_ARGUMENT == -1 && RSF_SCREEN_UNKNOWN == 0 &&
    RSF_SCREEN_MENU == 1 && RSF_SCREEN_FLIGHT == 4 && RSF_GAME_RENDER_SR == 1 && RSF_GAME_RENDER_WINDOW == 9 &&
    RSF_GAME_RENDER_FINAL_SCENE == 10, "native/d3d12.rs mirrors these Runtime constants");

/* The compositor effect records, as native/platform.rs mirrors them. */
_Static_assert(sizeof(rsf_godot_effect_frame) == 272 && offsetof(rsf_godot_effect_frame, motion_valid) == 36 &&
    offsetof(rsf_godot_effect_frame, camera_transform) == 48 &&
    offsetof(rsf_godot_effect_frame, jittered_projection) == 144 && sizeof(rsf_godot_effect_resources) == 24 &&
    sizeof(rsf_godot_effect_status) == 416 && offsetof(rsf_godot_effect_status, frame) == 16 &&
    offsetof(rsf_godot_effect_status, resources) == 288 && offsetof(rsf_godot_effect_status, buffers_available) == 312 &&
    offsetof(rsf_godot_effect_status, captures) == 320 && offsetof(rsf_godot_effect_status, capture_max_abs_uv) == 368 &&
    offsetof(rsf_godot_effect_status, stage) == 376 && offsetof(rsf_godot_effect_status, rendering_ready) == 400 &&
    sizeof(rsf_godot_effect_readback) == 24 && offsetof(rsf_godot_effect_readback, groups_y) == 16 &&
    sizeof(rsf_godot_effect_leaves) == 32 && offsetof(rsf_godot_effect_leaves, read_motion) == 24,
    "native/platform.rs mirrors these layouts");

_Static_assert(sizeof(rsf_godot_vk_features) == 56 && offsetof(rsf_godot_vk_features, capture) == 24,
    "rsf_godot_vk_features");

const char* rsf_godot_version(void)
{
    return RSF_VERSION_STRING;
}

/* These symbols only exist in configurations which compile the corresponding provider. Supply raw
   build facts and optional functions to the Rust owner. */
const rsf_godot_vk_features* rsf_godot_vk_native_features(void)
{
    static const rsf_godot_vk_features features = {
#if defined(RSF_VULKAN_SR_ENABLED)
        1u,
#else
        0u,
#endif
#if defined(RSF_VULKAN_FSR4_ENABLED)
        1u,
#else
        0u,
#endif
#if defined(RSF_VULKAN_XESS_FG_ENABLED)
        1u,
#else
        0u,
#endif
#if defined(RSF_VULKAN_FSR4_FG_ENABLED)
        rsf_fsr4gvk_probe,
#else
        NULL,
#endif
#if defined(RSF_FRAME_DUMP_VULKAN)
        {rsf_input_dump_vk_create, rsf_input_dump_vk_snapshot, rsf_input_dump_vk_drain, rsf_input_dump_vk_destroy},
#else
        {NULL, NULL, NULL, NULL},
#endif
    };
    return &features;
}

void rsf_godot_host_unload(void);

/* Rust statics need an explicit release at the DLL's CRT boundary. The hook registers with atexit
   from a CRT initializer, which is what the former C++ static destructor did, so the CRT runs it
   at DLL detach and at process exit. */
static void host_unload_at_exit(void)
{
    rsf_godot_host_unload();
}

#if defined(_MSC_VER)
static void __cdecl register_host_unload(void)
{
    atexit(host_unload_at_exit);
}

#pragma section(".CRT$XCU", read)
__declspec(allocate(".CRT$XCU")) static void(__cdecl* const register_host_unload_hook)(void) =
    register_host_unload;
#else
__attribute__((constructor)) static void register_host_unload(void)
{
    atexit(host_unload_at_exit);
}
#endif
