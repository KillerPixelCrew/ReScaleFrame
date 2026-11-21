// SPDX-License-Identifier: MIT
#pragma once
#include "native_geometry.h"
#include <cstddef>
#include <cstdint>

namespace rsf::anomaly::geometry {

// Hands lifetime retirements from the engine lifecycle thread to the render-thread TemporalHistory owner.
// TemporalHistory has a single owner, so the native sink must never call it directly.
class RetirementQueue {
public:
    RetirementQueue();
    ~RetirementQueue();
    RetirementQueue(const RetirementQueue&) = delete;
    RetirementQueue& operator=(const RetirementQueue&) = delete;
    // GeometrySink::retired target. Thread-safe, noexcept; an allocation failure is counted, not thrown.
    static void sink(void* queue, temporal::InstanceKey key) noexcept;
    // Render thread only. Applies every queued retirement to the history that owns the frame.
    std::size_t drain(void (*apply)(void* user, temporal::InstanceKey), void* user);
    // Retirements that could not be queued. Stale snapshots stay harmless because the new generation
    // never equals an old key and the registry refuses retired draws, but the count is evidence.
    std::uint64_t dropped() const noexcept;
private:
    rsf_anomaly_geometry_retirements* state_;
};

// Borrowed from the guarded draw capture for this call. 255 denotes missing view or domain.
struct SnapshotFacts {
    rsf_anomaly_geometry_key key;
    const temporal::Mat4* world;
    const temporal::Mat4* bones;
    std::size_t bone_count;
    std::uint64_t palette_layout;
    std::uint8_t view, domain, draw_kind;
};
SnapshotFacts snapshot_facts(const DrawCapture&) noexcept;

}
