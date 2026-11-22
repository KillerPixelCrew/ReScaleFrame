/* SPDX-License-Identifier: MIT */
/* Binds the integrated XeSS bridge's shared-image and fence exports to the V03
   importer. vulkan_xess_fg.rs admits the setup and maps the results. */
#include "native_fg_xess_vulkan_platform.h"

static int32_t create_image(void* user, uint32_t width, uint32_t height, uint32_t format, void** handle,
    void** resource) {
    struct ID3D12Resource* image = 0;
    const int32_t result = rsf_vk_xess_fg_create_shared_image((rsf_xess_bridge*)user, width, height, format,
        handle, &image);
    *resource = image;
    return result;
}

static void release_image(void* resource) {
    rsf_vk_xess_fg_release_image((struct ID3D12Resource*)resource);
}

static int32_t create_fence(void* user, void** handle) {
    return rsf_vk_xess_fg_create_shared_fence((rsf_xess_bridge*)user, handle);
}

static const rsf_xessvk_import_create operations = {
    RsfXessBridgeAdapterLuid, rsf_dx12vk_interop_create, create_image, release_image, create_fence,
};

int32_t rsf_xessvk_interop_create(const rsf_srvk_setup* setup, rsf_xess_bridge* bridge,
    uint32_t timeline_enabled, rsf_xessvk_interop** output) {
    return rsf_core_xessvk_interop_create(&operations, setup, bridge, timeline_enabled, output);
}
