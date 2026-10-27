// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstddef>
#include <cstdint>
#include <cstring>
inline constexpr uint32_t rsf_rdr2_dlss_modes[] = {2, 2, 1, 0, 3};
inline uint32_t rsf_rdr2_dlss_choice(const uint32_t* modes, uint32_t count, uint32_t quality) noexcept
{
    if (!modes || !count || count > 7 || quality > 4) return UINT32_MAX;
    for (uint32_t i = 0; i < count; ++i)
        if (modes[i] == rsf_rdr2_dlss_modes[quality]) return i;
    return UINT32_MAX;
}
// Exact 1491.50 generated collection/parser layout. This changes only the native
// input carrier and marker pump, not RSF preferences or other graphics settings.
inline bool rsf_rdr2_pin_engine_settings(void* collection, size_t bytes, uint32_t vendor, uint32_t quality = 2) noexcept
{
    if (!collection || bytes < 0x180 || quality > 4) return false;
    auto* data = static_cast<unsigned char*>(collection);
    uint16_t version{}; std::memcpy(&version, data + 0x178, 2);
    // Native defaults have version0; a loaded 1491.50 collection has version37.
    // The adjacent two bytes are padding, not part of the serialized version.
    if (version != 0 && version != 37) return false;
    constexpr uint32_t indices[] = {2, 2, 3, 4, 5};
    // RAGE only maps FSR2 modes1..3 to the SDK. Native and Ultra Performance
    // keep an active carrier and override its dimension producer instead.
    constexpr uint32_t fsr_modes[] = {1, 1, 2, 3, 3};
    const uint32_t dlss = vendor == 0x10de ? indices[quality] : 0u;
    const uint32_t dlss_quality = vendor == 0x10de ? rsf_rdr2_dlss_modes[quality] : 5u;
    const uint32_t fsr = vendor == 0x10de ? 0u : fsr_modes[quality];
    const uint32_t markers = 1;
    std::memcpy(data + 0xe8, &dlss, 4);
    // dlssIndex is the displayed selector; dlssQuality is the renderer's
    // actual mode (Balanced1, disabled5). Pin both sides of that conversion.
    std::memcpy(data + 0xec, &dlss_quality, 4);
    std::memcpy(data + 0xf4, &fsr, 4);
    std::memcpy(data + 0x15c, &markers, 4);
    return true;
}
