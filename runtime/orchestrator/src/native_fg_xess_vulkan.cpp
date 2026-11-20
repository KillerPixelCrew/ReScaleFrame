// SPDX-License-Identifier: MIT
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include "native_fg_xess_vulkan_platform.h"

struct ID3D12Resource;
extern "C" int VulkanStoryXessFgCreateSharedImage(rsf_xess_bridge*, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" void VulkanStoryXessFgReleaseImage(ID3D12Resource*);
extern "C" int VulkanStoryXessFgCreateSharedFence(rsf_xess_bridge*, HANDLE*);

namespace {
constexpr rsf_xessvk_import_create operations{
    RsfXessBridgeAdapterLuid, rsf_dx12vk_interop_create,
    [](void* user, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource) -> int32_t {
        ID3D12Resource* image = nullptr;
        const auto result = VulkanStoryXessFgCreateSharedImage(static_cast<rsf_xess_bridge*>(user),
            width, height, format, handle, &image);
        *resource = image;
        return result;
    },
    [](void* resource) { VulkanStoryXessFgReleaseImage(static_cast<ID3D12Resource*>(resource)); },
    [](void* user, void** handle) -> int32_t {
        return VulkanStoryXessFgCreateSharedFence(static_cast<rsf_xess_bridge*>(user), handle);
    },
};
}

int32_t rsf_xessvk_interop_create(const rsf_srvk_setup* setup, rsf_xess_bridge* bridge,
    uint32_t timeline_enabled, rsf_xessvk_interop** output) {
    return rsf_core_xessvk_interop_create(&operations, setup, bridge, timeline_enabled, output);
}
int32_t rsf_xessvk_interop_image(rsf_xessvk_interop* owner, uint32_t format,
    uint32_t width, uint32_t height, rsf_xessvk_shared_image* output) {
    return rsf_core_xessvk_interop_result(rsf_dx12vk_interop_image(owner, format, width, height, output));
}
uint64_t rsf_xessvk_interop_timeline(const rsf_xessvk_interop* owner) {
    return rsf_dx12vk_interop_timeline(owner);
}
int32_t rsf_xessvk_interop_destroy(rsf_xessvk_interop* owner, uint32_t queues_drained) {
    return rsf_core_xessvk_interop_result(rsf_dx12vk_interop_destroy(owner, queues_drained));
}
