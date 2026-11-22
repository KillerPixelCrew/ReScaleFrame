// SPDX-License-Identifier: MIT
#pragma once
#include "vulkan_fg_resources.h"
#include <rescaleframe/native_fg_vulkan.h>
#include <vulkan/vulkan.h>

struct rsf_vkfg_device;
extern "C" {
extern const rsf_vkfg_platform rsf_core_vkfg_platform;
rsf_vkfg_device* rsf_core_vkfg_load_device(const rsf_srvk_setup*, const rsf_fgvk_setup*, bool with_view);
rsf_vkfg_device* rsf_core_vkfg_bind_images(void* device, const rsf_vk_function (*entries)[6]);
void rsf_core_vkfg_device_destroy(rsf_vkfg_device*);
void rsf_core_vkfg_device_memory(rsf_vkfg_device*, const VkPhysicalDeviceMemoryProperties*);
void rsf_core_vkfg_device_wait_timing(rsf_vkfg_device*, const char* stage, uint64_t frame);
}

namespace rsf::vkfg {
inline constexpr const auto& platform = rsf_core_vkfg_platform;

// Owns the Rust dispatch table. Images and fences are retired by the owner
// before it closes the native SDK leaf.
class Device {
    rsf_vkfg_device* state = nullptr;

    bool replace(rsf_vkfg_device* candidate) {
        if (!candidate) return false;
        rsf_core_vkfg_device_destroy(state);
        state = candidate;
        return true;
    }

public:
    Device() = default;
    ~Device() { rsf_core_vkfg_device_destroy(state); }
    Device(const Device&) = delete;
    Device& operator=(const Device&) = delete;

    bool load_images(const rsf_srvk_setup& setup, bool with_view) {
        return replace(rsf_core_vkfg_load_device(&setup, nullptr, with_view));
    }
    bool load(const rsf_fgvk_setup& setup, bool with_view) {
        return replace(rsf_core_vkfg_load_device(&setup.graphics, &setup, with_view));
    }
    bool bind_images(void* device, const rsf_vk_function (&entries)[6]) {
        return replace(rsf_core_vkfg_bind_images(device, &entries));
    }
    void memory_types(const VkPhysicalDeviceMemoryProperties& properties) {
        rsf_core_vkfg_device_memory(state, &properties);
    }
    void wait_timing(const char* stage, uint64_t frame) { rsf_core_vkfg_device_wait_timing(state, stage, frame); }
    void* get() const { return state; }
};
}
