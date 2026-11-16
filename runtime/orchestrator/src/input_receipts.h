// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <cstdint>
#include <span>
namespace rsf::latency {
// Caller serializes access. Permit bounded callback reordering without replay.
struct ReceiptWindow {
    uint64_t highest{}, seen{};
    bool accept(uint64_t id) noexcept {
        if (!id) return false;
        if (id>highest) {
            const auto distance=id-highest;
            seen=distance>=64 ? 1 : (seen<<distance)|1;
            highest=id;
            return true;
        }
        const auto distance=highest-id;
        if (distance>=64 || (seen&(uint64_t(1)<<distance))) return false;
        seen|=uint64_t(1)<<distance;
        return true;
    }
};
struct ReceiptOwner {
    uint64_t source{},session{},generation{},ready{},simulation_begin{};
    bool eligible{};
};
inline uint64_t receipt_owner(std::span<const ReceiptOwner> owners,uint64_t session,uint64_t generation,uint64_t timestamp) noexcept {
    uint64_t selected=0;
    for (const auto& owner:owners) {
        if (!owner.eligible || !owner.source || owner.session!=session || owner.generation!=generation ||
            !owner.ready || !owner.simulation_begin || owner.ready>timestamp || owner.simulation_begin>timestamp) continue;
        if (selected) return 0; // Ambiguity is not permission to choose the newest source.
        selected=owner.source;
    }
    return selected;
}
}
