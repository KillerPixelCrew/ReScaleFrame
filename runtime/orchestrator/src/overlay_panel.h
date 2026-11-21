/* SPDX-License-Identifier: MIT */
#pragma once

#include <rescaleframe/overlay.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The five entry points of rescaleframe_overlay.dll. */
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

/* If module is null, a successful load retains the module for the process.
   Otherwise it transfers a reference that must be released or retained. */
uint32_t rsf_core_overlay_panel_open(rsf_overlay_host_panel* out, uint32_t* error, void** module);
void rsf_core_overlay_panel_release(void* module);
void rsf_core_overlay_panel_retain(void* module);

#ifdef __cplusplus
}
#endif
