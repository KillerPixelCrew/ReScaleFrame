/* SPDX-License-Identifier: MIT */
/* XeSS Vulkan FG creation. vulkan_xess_fg.rs owns admission, the presenter, the other provider
   entries and retirement; this file binds the integrated bridge exports (bridge.c), which exist
   only in builds with the XeSS-FG SDK (RSF_HAVE_XESS_FG_VK, the xess-fg-vk Cargo feature). */
#include "native_fg_xess_vulkan_platform.h"

static const rsf_xessgvk_ops operations = {
    rsf_xessvk_interop_create, RsfXessBridgeCreate, RsfXessBridgeStart, RsfXessBridgeMaxGenerated,
    RsfXessBridgeSetGenerated, rsf_vk_xess_fg_set_enabled, RsfXessBridgePresent, RsfXessBridgeCaptureSource,
    RsfXessBridgeCapturedCallbacks, rsf_vk_xess_fg_wait_idle, rsf_vk_xess_fg_prepare_destroy,
    rsf_vk_xess_fg_destroy_checked, rsf_vk_xess_fg_sleep, rsf_vk_xess_fg_marker, rsf_vk_xess_fg_set_latency_mode,
};

int32_t rsf_xessgvk_create(const rsf_fgvk_setup* setup, const rsf_xessgvk_options* options,
    rsf_xessgvk** output, uint64_t* chain) {
    /* Rust admits the setup before it asks the shared graphics table to describe create_info. */
    return rsf_core_xessgvk_create(&operations, setup, options, output, chain);
}
