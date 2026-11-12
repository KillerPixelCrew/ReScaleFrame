// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "../temporal/history.h"
#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <optional>
#include <span>
#include <string_view>
#include <vector>
#include "native_hooks.h"

namespace rsf::anomaly::geometry {
using temporal::InstanceKey;
class IdentityRegistry {
public:
    std::optional<InstanceKey> acquire(std::uint64_t owner, std::uint64_t visual);
    std::optional<InstanceKey> find(std::uint64_t owner, std::uint64_t visual) const;
    std::optional<InstanceKey> find_visual(std::uint64_t visual) const;
    std::optional<InstanceKey> retire(std::uint64_t owner, std::uint64_t visual);
    std::vector<InstanceKey> retire_all();
private:
    mutable std::mutex mutex_;
    std::map<std::pair<std::uint64_t, std::uint64_t>, InstanceKey> live_;
    std::uint64_t generation_{};
};

enum class DrawKind : std::uint8_t { Rigid, HardwareSkinned, SingleBone, SoftwareSkinned, Tree };
struct DrawIdentity {
    InstanceKey key{};
    DrawKind kind{};
    std::optional<temporal::ViewId> view;
    std::optional<temporal::ProjectionDomain> domain;
    const float* world{}; // Borrowed native row-vector Fmatrix, read again at the actual D3D draw.
    const std::byte* skeleton{}; // Actual CSkeletonX subobject, not the complete visual.
};
struct DrawCapture {
    InstanceKey key{};
    DrawKind kind{};
    std::optional<temporal::ViewId> view;
    std::optional<temporal::ProjectionDomain> domain;
    std::optional<temporal::Mat4> world;
    std::uint64_t palette_layout{};
    std::vector<temporal::Mat4> bones;
};
class DrawContextScope {
public:
    DrawContextScope(std::optional<temporal::ViewId>, std::optional<temporal::ProjectionDomain>) noexcept;
    ~DrawContextScope();
    DrawContextScope(const DrawContextScope&) = delete;
    DrawContextScope& operator=(const DrawContextScope&) = delete;
private:
    friend class DrawScope;
    std::optional<temporal::ViewId> view_;
    std::optional<temporal::ProjectionDomain> domain_;
    DrawContextScope* previous_{};
    static thread_local DrawContextScope* current_;
};
class DrawScope {
public:
    DrawScope(const IdentityRegistry&, std::optional<InstanceKey>, DrawKind, const float*,
        const std::byte* skeleton = nullptr) noexcept;
    ~DrawScope();
    DrawScope(const DrawScope&) = delete;
    DrawScope& operator=(const DrawScope&) = delete;
private:
    friend const DrawIdentity* borrow_draw() noexcept;
    const IdentityRegistry& registry_;
    DrawIdentity identity_{};
    DrawScope* previous_{};
    static thread_local DrawScope* current_;
};
// Valid only until the matching visual returns, on this thread. No engine object/resource lease.
const DrawIdentity* borrow_draw() noexcept;
std::optional<DrawCapture> capture_current_draw() noexcept;

struct GeometrySink {
    void* context{};
    // Called synchronously after identity invalidation, before engine pooling/free or a new generation.
    // Runs on the engine lifecycle caller thread, not necessarily the render thread. Must not reenter.
    void (*retired)(void*, InstanceKey) noexcept{};
};
void geometry_set_sink(GeometrySink) noexcept; // Set only with native hooks quiescent.
// Monotonic process counters for live validation. They prove hook invocation, not temporal correctness.
struct GeometryStats {
    std::uint64_t models_registered{}, models_retired{}, draws_identified{}, draws_unidentified{}, capture_faults{};
};
GeometryStats geometry_stats() noexcept;
std::span<const HookSite> native_geometry_hook_sites() noexcept;
bool install_native_geometry(std::byte* module, std::uint32_t image_size,
    std::string_view verified_executable_sha256) noexcept;
bool disable_native_geometry() noexcept;
bool geometry_hooks_active() noexcept;
void geometry_retire_all() noexcept; // Device/session teardown. Generation never resets.
#ifdef RSF_ANOMALY_GEOMETRY_TESTING
void geometry_test_bind_module(std::byte*) noexcept;
#endif
}
