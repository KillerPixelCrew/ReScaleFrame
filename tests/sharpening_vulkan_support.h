// SPDX-License-Identifier: MIT
#pragma once
#include "../runtime/orchestrator/src/sharpening_vulkan.h"

namespace rsf::sharpening {
class Vulkan {
    rsf_sharpenvk_host host{};
public:
    Vulkan() = default;
    Vulkan(const Vulkan&) = delete;
    Vulkan& operator=(const Vulkan&) = delete;
    ~Vulkan() { rsf_sharpenvk_host_destroy(&host); }
    void clear() { rsf_sharpenvk_host_clear(&host); }
    void retire(void* commands) { rsf_sharpenvk_host_retire(&host, commands); }
    uint64_t dispatches() const { return rsf_sharpenvk_host_dispatches(&host); }
    bool run(const rsf_srvk_setup& setup, void* commands, const rsf_srvk_frame& input, VkImage output) {
        return rsf_sharpenvk_host_run(&host, &setup, commands, &input, reinterpret_cast<uint64_t>(output)) != 0;
    }
};
}
