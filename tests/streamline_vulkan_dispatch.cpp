// SPDX-License-Identifier: MIT
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include "../runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h"
#include "vulkan-fg/streamline_abi.h"

using namespace sl::chi::ngxDispatch;

extern "C" void rsf_fixture_sl_dispatch_configure(uintptr_t owner, uintptr_t resolve) {
    configure(reinterpret_cast<VkDevice>(owner), nullptr, reinterpret_cast<PFN_vkGetDeviceProcAddr>(resolve));
}
extern "C" uintptr_t rsf_fixture_sl_dispatch_get(uintptr_t handle, const char* name) {
    return reinterpret_cast<uintptr_t>(GetDeviceProcAddr(reinterpret_cast<VkDevice>(handle), name));
}
extern "C" uintptr_t rsf_fixture_sl_dispatch_fill() {
    return reinterpret_cast<uintptr_t>(FillBuffer);
}
// First-use state of an SDK-created image, as CreateImage records it.
extern "C" void rsf_fixture_sl_dispatch_fresh(uintptr_t image, uint32_t mips, uint32_t layers, uint32_t aspects) {
    fresh[reinterpret_cast<VkImage>(image)] = {mips, layers, aspects,
        std::vector<VkImageAspectFlags>(size_t(mips) * layers)};
}
extern "C" uint32_t rsf_fixture_sl_dispatch_tracked(uintptr_t image) {
    return fresh.count(reinterpret_cast<VkImage>(image)) ? 1u : 0u;
}
// The barriers the corrected dispatch records for one requested image barrier.
extern "C" uint32_t rsf_fixture_sl_dispatch_barriers(const void* requested, void* output, uint32_t capacity) {
    const auto barriers = initializedBarriers(1, static_cast<const VkImageMemoryBarrier*>(requested));
    for (size_t i = 0; i < barriers.size() && i < capacity; ++i)
        static_cast<VkImageMemoryBarrier*>(output)[i] = barriers[i];
    return static_cast<uint32_t>(barriers.size());
}
int main() { return rsf_fixture_sl_dispatch_run(); }
