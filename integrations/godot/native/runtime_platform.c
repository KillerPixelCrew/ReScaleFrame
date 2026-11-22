/* SPDX-License-Identifier: MIT */
/* The native side of ReScaleFrame.Godot.Native.dll that Rust cannot express: layout checks of the
   engine-module records against the C headers, the CMake-selected optional Vulkan provider symbols, the
   CMake project version and the DLL unload hook. The GDExtension classes, every rsfg_* export and
   all decisions are Rust (native/lib.rs). */
#include <rescaleframe/native_cpu.h>
#include "api.h"
#include <rescaleframe/native_cpu_service.h>
#include "vulkan_features.h"

#include <rescaleframe/fg_choice.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/native_latency_vulkan.h>
#include <rescaleframe/version.h>
#include <windows.h>
#include <stddef.h>
#include <stdlib.h>

/* The engine module's records and the Runtime constants native/*.rs mirrors. */
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
