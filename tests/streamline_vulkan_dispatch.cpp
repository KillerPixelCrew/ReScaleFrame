// SPDX-License-Identifier: MIT
#define VK_NO_PROTOTYPES
#include <vulkan/vulkan.h>
#include "../runtime/vulkan/streamline/sdk-fixes/ngx-vulkan-dispatch.h"
#include "vulkan-fg/streamline_abi.h"

using namespace sl::chi::ngxDispatch;

extern "C" void rsf_fixture_sl_dispatch_configure(uintptr_t owner, uintptr_t resolve) {
    configure(reinterpret_cast<VkDevice>(owner), nullptr, reinterpret_cast<PFN_vkGetDeviceProcAddr>(resolve));
}
extern "C" uintptr_t rsf_fixture_sl_dispatch_get(uintptr_t device, const char* name) {
    return reinterpret_cast<uintptr_t>(GetDeviceProcAddr(reinterpret_cast<VkDevice>(device), name));
}
extern "C" uintptr_t rsf_fixture_sl_dispatch_fill() {
    return reinterpret_cast<uintptr_t>(FillBuffer);
}
int main() { return rsf_fixture_sl_dispatch_run(); }
