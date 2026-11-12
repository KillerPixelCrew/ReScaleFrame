// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// The shared render-frontend SR settings (INI keys SR, SRBackend, SRQuality, DLSSPreset) as the Anomaly SR stages apply
// them. No dependencies, so the overlay service and its fixtures include it without the SR pipeline headers.
#include <cstdint>

namespace rsf::anomaly::sr {
inline constexpr uint32_t backend_dlss = 1, backend_fsr3 = 3, backend_fsr4 = 4, backend_xess = 5;   // RSF_SR_*
inline constexpr uint32_t adapter_backends = (1u << backend_dlss) | (1u << backend_fsr3) | (1u << backend_fsr4) | (1u << backend_xess);
// Native, Quality, Balanced, Performance. Ultra performance renders a third of the output, below the half-size
// floor of the world window (sr_plan.h min_scale, the engine's rt_half_depth halving).
inline constexpr uint32_t adapter_qualities = 0x0fu;
inline constexpr bool adapter_backend(uint32_t backend) noexcept { return backend < 32 && (adapter_backends >> backend) & 1u; }
inline constexpr bool adapter_quality(uint32_t quality) noexcept { return quality < 32 && (adapter_qualities >> quality) & 1u; }
struct SrSettings {
    bool enabled{true};
    uint32_t backend{backend_dlss};
    uint32_t quality{1};   // Quality
    uint32_t preset{};     // DLSSPreset: the user's choice, forwarded unchanged (0 = the DLSS default)
    bool operator==(const SrSettings&) const = default;
};
// A request the adapter cannot apply keeps the current value for that field.
inline SrSettings sanitize(const SrSettings& wanted, const SrSettings& current) noexcept {
    SrSettings out = wanted;
    if (!adapter_backend(out.backend)) out.backend = current.backend;
    if (!adapter_quality(out.quality)) out.quality = current.quality;
    return out;
}
}
