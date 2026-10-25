// SPDX-License-Identifier: GPL-3.0-only
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include "native_fg_xess_vulkan.h"

struct ID3D12Resource;
extern "C" int VulkanStoryXessFgCreateSharedImage(rsf_xess_bridge*, uint32_t,
    uint32_t, uint32_t, HANDLE*, ID3D12Resource**);
extern "C" void VulkanStoryXessFgReleaseImage(ID3D12Resource*);
extern "C" int VulkanStoryXessFgCreateSharedFence(rsf_xess_bridge*, HANDLE*);

namespace {
int32_t xess_result(int32_t code) {
    if (code == RSF_DX12VK_INTEROP_INVALID_ARGUMENT) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    if (code == RSF_DX12VK_INTEROP_ABI_MISMATCH) return RSF_XESS_BRIDGE_ABI_MISMATCH;
    return code;
}
}
int32_t rsf_xessvk_interop_create(const rsf_srvk_setup* setup, rsf_xess_bridge* bridge,
    uint32_t timeline_enabled, rsf_xessvk_interop** output) {
    if (!output) return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    *output = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !bridge || timeline_enabled != 1 ||
        !setup->instance || !setup->physical_device || !setup->device || !setup->instance_proc || !setup->device_proc)
        return RSF_XESS_BRIDGE_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION) return RSF_XESS_BRIDGE_ABI_MISMATCH;
    const rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), RSF_DX12VK_INTEROP_ABI_VERSION,
        RsfXessBridgeAdapterLuid(bridge), bridge,
        [](void* user, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource) -> int32_t {
            ID3D12Resource* image = nullptr;
            const int32_t code = VulkanStoryXessFgCreateSharedImage(static_cast<rsf_xess_bridge*>(user),
                width, height, format, handle, &image);
            *resource = image; return code;
        },
        [](void* resource) { VulkanStoryXessFgReleaseImage(static_cast<ID3D12Resource*>(resource)); },
        [](void* user, void** handle) -> int32_t {
            return VulkanStoryXessFgCreateSharedFence(static_cast<rsf_xess_bridge*>(user), handle);
        }};
    return xess_result(rsf_dx12vk_interop_create(setup, &callbacks, timeline_enabled, output));
}
int32_t rsf_xessvk_interop_image(rsf_xessvk_interop* self, uint32_t format,
    uint32_t width, uint32_t height, rsf_xessvk_shared_image* output) {
    return xess_result(rsf_dx12vk_interop_image(self, format, width, height, output));
}
uint64_t rsf_xessvk_interop_timeline(const rsf_xessvk_interop* self) {
    return rsf_dx12vk_interop_timeline(self);
}
int32_t rsf_xessvk_interop_destroy(rsf_xessvk_interop* self, uint32_t queues_drained) {
    return xess_result(rsf_dx12vk_interop_destroy(self, queues_drained));
}
