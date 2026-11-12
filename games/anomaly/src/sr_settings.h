// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// The shared render-frontend SR settings (INI keys SR, SRBackend, SRQuality, DLSSPreset) as the Anomaly SR stages apply
// them. No dependencies, so the overlay service and its fixtures include it without the SR pipeline headers.
#include <cstdint>
#include <string>

namespace rsf::anomaly::sr {
// A vendor runtime directory as Streamline, FidelityFX and XeSS can use it. The INI path the carrier hands over can be a
// Win32 long-path name ("\\?\D:\...", "\\?\UNC\server\share\..."). Streamline appends "/sl.common.dll" to its plugin
// directory, and a "\\?\" name disables the slash conversion, so every plugin failed to load and Stage 5 stayed on the
// bilinear seam (wave 7 run, c1 log). Strips the prefix, uses backslashes only and drops trailing separators.
inline std::wstring plain_directory(std::wstring path) {
    if (path.starts_with(LR"(\\?\UNC\)")) path = LR"(\\)" + path.substr(8);
    else if (path.starts_with(LR"(\\?\)") || path.starts_with(LR"(\??\)")) path = path.substr(4);
    for (auto& c : path) if (c == L'/') c = L'\\';
    while (path.size() > 3 && path.back() == L'\\') path.pop_back();
    return path;
}
inline constexpr uint32_t backend_dlss = 1, backend_fsr3 = 3, backend_fsr4 = 4, backend_xess = 5;   // RSF_SR_*
inline constexpr uint32_t adapter_backends = (1u << backend_dlss) | (1u << backend_fsr3) | (1u << backend_fsr4) | (1u << backend_xess);
// Native, Quality, Balanced, Performance, Ultra performance (a third of the output, inside sr_plan.h min_scale).
inline constexpr uint32_t adapter_qualities = 0x1fu;
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
