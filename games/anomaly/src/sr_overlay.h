// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// The link between the SR stages and the shared overlay service, without a link-time dependency in either direction
// (the overlay fixtures build overlay_service.cpp without the SR stages and vice versa). sr::configure publishes the
// table; overlay::start reads it once, cold, before the panel exists. Null means SR is unavailable in this adapter.
#include "sr_settings.h"
#include <atomic>
#include <cstdint>

struct rsf_overlay_sr_report;

namespace rsf::anomaly::sr {
// With SRStage=5 the adapter offers the backends whose vendor runtime directory holds the runtime DLL, and qualities
// Native to Performance; otherwise nothing.
struct OverlayChoices {
    uint32_t backends{}, qualities{};
    bool preset{};
    SrSettings defaults{};
};
struct OverlayLink {
    OverlayChoices (*choices)() noexcept;
    // Any thread: what reached the last presented frame; zero when SR produced nothing.
    int (*report)(rsf_overlay_sr_report*) noexcept;
    // Any thread: shared settings to apply at the next frame boundary.
    void (*request)(const SrSettings&) noexcept;
};
inline std::atomic<const OverlayLink*> overlay_link{nullptr};
}
