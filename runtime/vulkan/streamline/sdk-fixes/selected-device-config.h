// SPDX-License-Identifier: GPL-3.0-only
#pragma once

#include <cstdint>
#include "include/sl_core_types.h"

namespace rsf::streamline {

using FindAdapter = sl::Result(const sl::AdapterInfo&, uint32_t);

// The common plugin owns the adapter-bit numbering. Use its published lookup,
// passing only the selected Vulkan device's LUID. Passing vkPhysicalDevice would
// also permit the SDK's weaker device-ID match on identical-model adapters.
inline uint32_t selectedAdapterMask(const uint8_t* luid, uint32_t size, FindAdapter* findAdapter)
{
    if (!luid || size != 8 || !findAdapter) return 0;
    sl::AdapterInfo adapter{};
    adapter.deviceLUID = const_cast<uint8_t*>(luid);
    adapter.deviceLUIDSizeInBytes = size;
    uint32_t selected = 0;
    for (uint32_t index = 0; index < 32; ++index)
    {
        const uint32_t bit = uint32_t{1} << index;
        if (findAdapter(adapter, bit) != sl::Result::eOk) continue;
        if (selected) return 0; // Ambiguous identity must not merge two adapters.
        selected = bit;
    }
    return selected;
}

inline bool configApplies(uint32_t supportedAdapters, uint32_t selectedAdapter)
{
    return selectedAdapter && !(selectedAdapter & (selectedAdapter - 1)) &&
        (supportedAdapters & selectedAdapter) != 0;
}

inline bool deviceConfigApplies(sl::Feature feature, uint32_t supportedAdapters,
    uint32_t selectedAdapter, uint32_t selectedVendor)
{
    // Reflex intentionally supports all adapters for statistics, but its Vulkan
    // device requirements are NVIDIA-only and are generated from any local GPU.
    if (feature == sl::kFeatureReflex && selectedVendor != 0x10de) return false;
    return configApplies(supportedAdapters, selectedAdapter);
}

} // namespace rsf::streamline
