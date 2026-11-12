#pragma once

#include <cstdint>
#include <optional>

namespace rsf::anomaly::temporal {

struct Extent {
    std::uint32_t width{};
    std::uint32_t height{};
    bool operator==(const Extent&) const = default;
};

// Adapter-local values, not a DLL ABI. No Device globals are changed.
struct ResolutionPlan {
    Extent output;
    Extent render;
    Extent pda;
    bool operator==(const ResolutionPlan&) const = default;
};

inline bool valid_extent(Extent size)
{
    // D3D11 2D texture limit; 2x2 avoids degenerate temporal/jitter denominators.
    return size.width >= 2 && size.height >= 2 && size.width <= 16384 && size.height <= 16384;
}

inline std::optional<ResolutionPlan> plan_resolutions(Extent output, Extent render, Extent pda)
{
    if (!valid_extent(output) || !valid_extent(render) || !valid_extent(pda) ||
        render.width > output.width || render.height > output.height) {
        return std::nullopt;
    }
    return ResolutionPlan{output, render, pda};
}

} // namespace rsf::anomaly::temporal
