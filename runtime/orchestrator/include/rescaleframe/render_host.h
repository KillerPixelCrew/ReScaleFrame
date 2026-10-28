// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Called outside DllMain. The runtime owns settings, backend contexts and GPU retirement.
   The returned services and their user owner stay alive until process termination. The caller
   selects/prepares the game plugin before any graphics context is created. */
#define RSF_RENDER_HOST_ABI_VERSION 1u
#define RSF_RENDER_HOST_OWNS_PRESENTATION 0u
#define RSF_RENDER_HOST_ENGINE_PRESENTATION 1u
typedef struct rsf_render_host_setup {
    uint32_t struct_size, abi_version;
    const wchar_t* configuration_path;
    const wchar_t* runtime_directory;
    const wchar_t* configuration_section;
    uint32_t presentation_owner;
    uint32_t sr_backend_choices;
    uint32_t engine_type, application_id;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    uint32_t depth_inverted;
    float units_to_meters;
    uint32_t ui_mode;
    int (*accept_window)(void* user, void* native_window);
    void* user;
} rsf_render_host_setup;
/* The adapter supplies engine identity, configuration mapping and window
   selection. This host contains no game detection, signatures or policies. */
RSF_RUNTIME_API int32_t rsf_render_host_create(const rsf_render_host_setup*, rsf_game_host_services* services);
/* Reports the engine's actual presentation queue without manufacturing a
   submission event. The host retains the queue for its shared overlay. */
RSF_RUNTIME_API int32_t rsf_render_host_graphics(void* host_user, void* presentation_queue);
RSF_RUNTIME_API int32_t rsf_render_host_device(void* host_user, void* engine_device);
RSF_RUNTIME_API int32_t rsf_render_host_factory(void* host_user, void* engine_factory);
RSF_RUNTIME_API uint32_t rsf_render_host_graphics_ready(void* host_user);
/* Returns one owned IUnknown reference to the physical graphics object. */
RSF_RUNTIME_API void* rsf_render_host_native_object(void* host_user, void* engine_object);
#ifdef __cplusplus
}
#endif
