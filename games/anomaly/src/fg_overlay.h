// SPDX-License-Identifier: MIT
#pragma once
// What the shared overlay (overlay_service.cpp) needs from frame generation, published by the stage once the
// presentation bridge is installed. Null: no bridge, FG is not available and the panel offers no FG choices.
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
#include <dxgi.h>
#include <atomic>
typedef struct IDXGISwapChain rsf_anomaly_fg_swap_chain;
#else
typedef void rsf_anomaly_fg_swap_chain;
#endif
typedef struct rsf_anomaly_fg_overlay_link {
    // True for a chain the bridge presents through that is not the engine-facing one (a provider's physical or proxy
    // chain). The panel never draws on those.
    bool (*foreign_chain)(rsf_anomaly_fg_swap_chain* chain);
    // The startup values, used as the panel's defaults when the INI has none.
    uint32_t enabled, backend, generated, latency, frame_limit_us;
} rsf_anomaly_fg_overlay_link;

#ifdef __cplusplus
namespace rsf::anomaly::fg {
using OverlayLink = rsf_anomaly_fg_overlay_link;
inline std::atomic<const OverlayLink*> overlay_link{};
}
#endif
