// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// What the shared overlay (overlay_service.cpp) needs from frame generation, published by fg_stage.cpp once the
// presentation bridge is installed. Null: no bridge, FG is not available and the panel offers no FG choices.
#include <dxgi.h>
#include <atomic>
#include <cstdint>

namespace rsf::anomaly::fg {
struct OverlayLink {
    // True for a chain the bridge presents through that is not the engine-facing one (a provider's physical or proxy
    // chain). The panel never draws on those.
    bool (*foreign_chain)(IDXGISwapChain* chain) noexcept;
    // The startup values, used as the panel's defaults when the INI has none.
    uint32_t enabled, backend, generated, latency, frame_limit_us;
};
inline std::atomic<const OverlayLink*> overlay_link{};
}
