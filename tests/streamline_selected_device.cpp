// SPDX-License-Identifier: GPL-3.0-only
#include <array>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include "../runtime/vulkan/streamline/sdk-fixes/selected-device-config.h"

namespace {
using Luid = std::array<uint8_t, 8>;
constexpr Luid amd{1, 2, 3, 4, 5, 6, 7, 8};
constexpr Luid nvidia{8, 7, 6, 5, 4, 3, 2, 1};
constexpr Luid identicalModel{2, 2, 3, 4, 5, 6, 7, 8};
bool malformedInfo = false;
bool ambiguous = false;
sl::Result findAdapter(const sl::AdapterInfo& info, uint32_t mask)
{
    // Model the common lookup with its public LUID-only input. Any accidental
    // vkPhysicalDevice would enable the upstream device-ID alternative.
    if (info.vkPhysicalDevice || !info.deviceLUID || info.deviceLUIDSizeInBytes != 8)
    {
        malformedInfo = true;
        return sl::Result::eErrorInvalidParameter;
    }
    const std::array<Luid, 3> adapters{amd, nvidia, identicalModel};
    for (uint32_t index = 0; index < adapters.size(); ++index)
    {
        if ((mask & (uint32_t{1} << index)) &&
            std::memcmp(adapters[index].data(), info.deviceLUID, 8) == 0)
            return sl::Result::eOk;
    }
    if (ambiguous && mask == 8) return sl::Result::eOk;
    return sl::Result::eErrorAdapterNotSupported;
}

bool require(bool value, const char* message)
{
    if (!value) std::fprintf(stderr, "%s\n", message);
    return value;
}
} // namespace

int main()
{
    using namespace rsf::streamline;
    bool ok = true;
    const uint32_t amdBit = selectedAdapterMask(amd.data(), 8, findAdapter);
    const uint32_t nvBit = selectedAdapterMask(nvidia.data(), 8, findAdapter);
    const uint32_t sameModelBit = selectedAdapterMask(identicalModel.data(), 8, findAdapter);
    ok &= require(amdBit == 1 && nvBit == 2 && sameModelBit == 4 && !malformedInfo,
        "Selected LUID must preserve common adapter numbering and distinguish identical models");
    // Reflex reports all adapters for stats even when its Vulkan requirements
    // are NVIDIA-only. DLSS/FG have the NVIDIA bit; common/PCL have all three.
    const std::array<sl::Feature, 6> features{
        sl::kFeatureDLSS, sl::kFeatureDLSS_G, sl::kFeatureReflex, 0xffff, sl::kFeaturePCL, 0xfffe};
    const std::array<uint32_t, 6> masks{2, 2, 7, 7, 7, 0};
    uint32_t amdRequirements = 0, nvRequirements = 0;
    for (uint32_t index = 0; index < masks.size(); ++index)
    {
        if (deviceConfigApplies(features[index], masks[index], amdBit, 0x1002))
            amdRequirements |= uint32_t{1} << index;
        if (deviceConfigApplies(features[index], masks[index], nvBit, 0x10de))
            nvRequirements |= uint32_t{1} << index;
    }
    ok &= require(amdRequirements == 0x18 && nvRequirements == 0x1f,
        "AMD must retain common/PCL only; NVIDIA must retain its supported requirements");
    ok &= require(!configApplies(7, 0) && !configApplies(7, 3) && !configApplies(0, 1),
        "Unknown, ambiguous and unsupported masks must refuse");
    const Luid unknown{9};
    ok &= require(selectedAdapterMask(unknown.data(), 8, findAdapter) == 0 &&
        selectedAdapterMask(nullptr, 8, findAdapter) == 0 &&
        selectedAdapterMask(amd.data(), 7, findAdapter) == 0 &&
        selectedAdapterMask(amd.data(), 8, nullptr) == 0,
        "Missing identity or lookup must refuse");
    ambiguous = true;
    ok &= require(selectedAdapterMask(amd.data(), 8, findAdapter) == 0,
        "Multiple matching adapter bits must refuse");
    if (ok) std::puts("selected-device requirement filtering passed");
    return ok ? 0 : 1;
}
