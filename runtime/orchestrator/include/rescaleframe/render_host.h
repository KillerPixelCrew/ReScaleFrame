// SPDX-License-Identifier: MIT
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
/* presentation_owner. OWNS installs frame generation with process-lifetime DXGI factory detours.
   ENGINE keeps the engine's own Streamline presentation and observes its factory, device, queue
   and chain through a MinHook observer. EXPLICIT installs no hook and no observer: frame
   generation is installed explicitly, the engine creates its swap chain at its own creation site
   through rsf_render_host_create_swapchain and calls Present on it itself, and it reports every
   boundary through the host services. An engine that creates its own native chain instead keeps
   SR, submission, latency and feedback; frame generation then has no chain to present on. */
#define RSF_RENDER_HOST_OWNS_PRESENTATION 0u
#define RSF_RENDER_HOST_ENGINE_PRESENTATION 1u
#define RSF_RENDER_HOST_EXPLICIT_PRESENTATION 2u
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
   selection. This host contains no game detection, signatures or policies.
   The services include render_config. A process_upscale pass flagged RECONFIGURE (output extent
   required, camera optional) plans the saved SR backend and quality at that output: the provider
   chooses the render extent, and its vendor sampling gives jitter phases and LOD bias. The pass
   keeps native evaluation. render_config then reports that plan, enabled at the planned extents,
   for the integration to apply. It reports native extents with enabled 0 when SR is off, the
   provider refused, or the settings changed since the plan; a new RECONFIGURE pass plans again.
   Without RECONFIGURE the engine keeps choosing its own render extents, as before. */
RSF_RUNTIME_API int32_t rsf_render_host_create(const rsf_render_host_setup*, rsf_game_host_services* services);
/* Reports the engine's actual presentation queue without manufacturing a
   submission event. The host retains the queue for its shared overlay. ENGINE and EXPLICIT only. */
RSF_RUNTIME_API int32_t rsf_render_host_graphics(void* host_user, void* presentation_queue);
/* ENGINE: attach the observer to the engine's device or factory. EXPLICIT: accepted with nothing
   to attach (the creation call names the factory and queue). OWNS: RSF_ERROR_NOT_READY. */
RSF_RUNTIME_API int32_t rsf_render_host_device(void* host_user, void* engine_device);
RSF_RUNTIME_API int32_t rsf_render_host_factory(void* host_user, void* engine_factory);
/* EXPLICIT only, at the engine's serialized swap-chain creation site, after the device and its
   DIRECT presentation queue exist. All inputs are borrowed: an IDXGIFactory2, that queue, the
   HWND accept_window admits, and a DXGI_SWAP_CHAIN_DESC1. On success swapchain owns one
   IDXGISwapChain1 reference: the engine calls Present, ResizeBuffers and the colour-space calls on
   it and reports Present through the host services. Returns RSF_ERROR_INVALID_ARGUMENT for another
   mode, a missing argument, a queue that is not DIRECT or a refused window; RSF_ERROR_NOT_READY when
   frame generation was not installed, in which case the engine creates its own chain; otherwise
   the HRESULT of the creation. */
RSF_RUNTIME_API int32_t rsf_render_host_create_swapchain(void* host_user, void* factory,
    void* presentation_queue, void* window, const void* description, void** swapchain);
RSF_RUNTIME_API uint32_t rsf_render_host_graphics_ready(void* host_user);
/* Returns one owned IUnknown reference to the physical graphics object. */
RSF_RUNTIME_API void* rsf_render_host_native_object(void* host_user, void* engine_object);
#ifdef __cplusplus
}
#endif
