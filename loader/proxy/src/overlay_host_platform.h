/* SPDX-License-Identifier: MIT */
/* Native leaves of the shared D3D11 overlay host.

   The Rust owner in runtime/orchestrator/src/overlay_host.rs decides everything: lifecycle,
   ownership of the panel, renderer and device, the startup hint, visibility, texture sequencing,
   draw admission, failure handling and frame timing. Each entry here is one loader, COM, input,
   renderer or Win32 call with no policy of its own. Private to overlay_host.cpp and its Rust
   owner; the public entry points stay in overlay_host.h. */

#ifndef RSF_PROXY_OVERLAY_HOST_PLATFORM_H
#define RSF_PROXY_OVERLAY_HOST_PLATFORM_H

#include <rescaleframe/overlay.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*rsf_overlay_host_log)(void* user, const char* message);

/* The five entry points of rescaleframe_overlay.dll. The module stays loaded for the process. */
typedef struct rsf_overlay_host_panel {
    rsf_overlay* (*create)(uint32_t abi_version);
    void (*destroy)(rsf_overlay* overlay);
    rsf_overlay_result (*frame)(rsf_overlay* overlay, const rsf_overlay_input* input,
                                const rsf_overlay_stats* stats, rsf_overlay_draw_data* draw_data,
                                rsf_overlay_intent* intent);
    uint32_t (*texture_updates)(rsf_overlay* overlay, rsf_overlay_texture_update* updates,
                                uint32_t max_updates);
    uint32_t (*textures_to_free)(rsf_overlay* overlay, uint64_t* ids, uint32_t max_ids);
} rsf_overlay_host_panel;

/* Why the panel DLL is not available. `error` is GetLastError for a failed load. */
enum {
    RSF_OVERLAY_HOST_PANEL_LOADED = 0,
    RSF_OVERLAY_HOST_PANEL_NO_PATH = 1,
    RSF_OVERLAY_HOST_PANEL_NOT_LOADED = 2,
    RSF_OVERLAY_HOST_PANEL_MISSING_EXPORTS = 3
};

typedef struct rsf_overlay_host_texture_desc {
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t bind_flags;
} rsf_overlay_host_texture_desc;

/* The output-merger targets bound before the overlay binds its own. Saved slots carry one
   reference each, which the owner releases after putting them back. */
enum { RSF_OVERLAY_HOST_TARGET_SLOTS = 8 };
typedef struct rsf_overlay_host_targets {
    void* views[RSF_OVERLAY_HOST_TARGET_SLOTS];
    void* depth;
} rsf_overlay_host_targets;

/* Every returned COM pointer transfers one reference. HRESULTs are returned as int32_t. */
typedef struct rsf_overlay_host_platform {
    uint32_t (*load_panel)(rsf_overlay_host_panel* out, uint32_t* error);

    /* IDXGISwapChain: its ID3D11Device, back buffer 0 as ID3D11Texture2D, and output window. */
    int32_t (*chain_device)(void* chain, void** device);
    int32_t (*chain_buffer)(void* chain, void** texture);
    void* (*chain_window)(void* chain);

    /* COM identity through IUnknown. Null never matches. */
    uint32_t (*same_object)(void* first, void* second);
    void* (*immediate_context)(void* device);
    void* (*texture_device)(void* texture);
    void (*texture_desc)(void* texture, rsf_overlay_host_texture_desc* out);
    int32_t (*create_view)(void* device, void* texture, void** view);
    void (*save_targets)(void* context, rsf_overlay_host_targets* out);
    void (*bind_target)(void* context, void* view);
    void (*restore_targets)(void* context, const rsf_overlay_host_targets* saved);

    /* overlay_renderer: created against a D3D11 device. */
    int32_t (*renderer_create)(void* device, rsf_overlay_host_log log, void* user, void** out);
    int32_t (*renderer_upload)(void* renderer, void* context,
                               const rsf_overlay_texture_update* update);
    void (*renderer_free)(void* renderer, uint64_t id);
    int32_t (*renderer_draw)(void* renderer, void* context, const rsf_overlay_draw_data* data,
                             uint32_t width, uint32_t height);
    void (*renderer_destroy)(void* renderer);

    /* overlay_input: the window subclass and the visibility it owns. */
    int32_t (*input_install)(void* window, uint32_t toggle_key, rsf_overlay_host_log log,
                             void* user);
    void (*input_uninstall)(void);
    uint32_t (*input_visible)(void);
    void (*input_set_visible)(uint32_t visible);
    void (*input_collect)(rsf_overlay_input* out, uint32_t width, uint32_t height,
                          float delta_seconds);

    /* GetTickCount64, and QueryPerformanceCounter/Frequency (non-zero on success). */
    uint64_t (*tick)(void);
    uint32_t (*counter)(int64_t* value);
    uint32_t (*frequency)(int64_t* value);
    uint32_t (*thread_id)(void);
} rsf_overlay_host_platform;

int rsf_core_overlay_host_start(const rsf_overlay_host_platform*, void* swapchain,
                                rsf_overlay_host_log log, void* user);
int rsf_core_overlay_host_start_device(const rsf_overlay_host_platform*, void* device, void* hwnd,
                                       rsf_overlay_host_log log, void* user);
int rsf_core_overlay_host_draw_target(const rsf_overlay_host_platform*, void* context,
                                      void* target, uint32_t width, uint32_t height,
                                      const rsf_overlay_stats* stats, rsf_overlay_intent* intent);
unsigned int rsf_core_overlay_host_visible(const rsf_overlay_host_platform*);
void rsf_core_overlay_host_toggle(const rsf_overlay_host_platform*);
int rsf_core_overlay_host_present(const rsf_overlay_host_platform*, void* swapchain,
                                  const rsf_overlay_stats* stats, rsf_overlay_intent* intent);
void rsf_core_overlay_host_stop(const rsf_overlay_host_platform*);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_PROXY_OVERLAY_HOST_PLATFORM_H */
