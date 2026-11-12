#pragma once

#include "math.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <compare>
#include <cstdint>
#include <limits>
#include <map>
#include <memory>
#include <optional>
#include <utility>
#include <vector>

namespace rsf::anomaly::temporal {

enum class ViewId : std::uint8_t { Main, Scope };
enum class ProjectionDomain : std::uint8_t { World, FirstPerson };

struct CameraState {
    Mat4 view{};
    Mat4 projection{}; // Unjittered, not combined. Defaults intentionally fail validation.
    Vec2 jitter_pixels{};
    bool operator==(const CameraState&) const = default;
};

struct FrameInput {
    ViewId view{ViewId::Main};
    // Extended monotonic SOURCE render counter, not Present count. Gaps are normal.
    std::uint64_t source_frame{};
    ResolutionPlan resolutions{};
    CameraState world{};
    std::optional<CameraState> first_person{};
    bool camera_cut{};
    bool first_person_cut{};
    bool operator==(const FrameInput&) const = default;
};

struct FrameToken {
    ViewId view{};
    std::uint64_t source_frame{};
    std::uint64_t serial{};
    std::uint64_t owner{};
    bool operator==(const FrameToken&) const = default;
};

enum class BeginStatus { Started, Repeated, Invalid, OutOfOrder, Conflict };
struct BeginResult {
    BeginStatus status{BeginStatus::Invalid};
    std::optional<FrameToken> token{};
};
struct CameraPair {
    CameraState current;
    std::optional<CameraState> previous;
    std::uint64_t source_frame{};
    std::optional<std::uint64_t> previous_source_frame;
};

// A raw address, grass batch index or cache slot alone is not a lifetime identity.
struct InstanceKey {
    std::uint64_t owner{};
    std::uint64_t visual{};
    std::uint64_t generation{};
    auto operator<=>(const InstanceKey&) const = default;
};

enum class InstanceKind : std::uint8_t { Rigid, Skinned, Deformed, Detail };
struct DeformationState {
    // Adapter-defined verified shader constant schema, including time/wind/wave parameters.
    std::uint64_t layout{};
    std::vector<Vec4> constants;
    bool operator==(const DeformationState&) const = default;
};
struct DetailState {
    // Per-item raster fade/scale, not a batch value. World includes item placement/rotation.
    double fade_scale{1};
    bool operator==(const DetailState&) const = default;
};
struct InstanceSnapshot {
    Mat4 world{}; // Must be populated by the actual producer, never defaulted to identity.
    ProjectionDomain domain{ProjectionDomain::World};
    InstanceKind kind{InstanceKind::Rigid};
    // Final local-space skinning matrices in shader palette order, copied before reuse.
    std::uint64_t palette_layout{};
    std::vector<Mat4> bones;
    std::optional<DeformationState> deformation;
    std::optional<DetailState> detail;
    bool operator==(const InstanceSnapshot&) const = default;
};
enum class CaptureStatus { Captured, Repeated, Invalid, StaleFrame, Conflict, Retired, MissingProjection };
struct CaptureResult {
    CaptureStatus status{CaptureStatus::Invalid};
    std::shared_ptr<const InstanceSnapshot> current;
    std::shared_ptr<const InstanceSnapshot> previous;
};

// Single render-thread owner. Adapter-local C++ types must not cross the runtime DLL ABI.
class TemporalHistory {
public:
    TemporalHistory() = default;
    TemporalHistory(const TemporalHistory&) = delete;
    TemporalHistory& operator=(const TemporalHistory&) = delete;
    TemporalHistory(TemporalHistory&&) = delete;
    TemporalHistory& operator=(TemporalHistory&&) = delete;

    BeginResult begin_frame(const FrameInput& input)
    {
        if (!valid(input) || owner_ == 0) {
            return {BeginStatus::Invalid, std::nullopt};
        }
        auto& state = views_.at(static_cast<std::size_t>(input.view));
        if (state.current && input.source_frame == state.current->source_frame) {
            return input == *state.current ? BeginResult{BeginStatus::Repeated, token(state)}
                : BeginResult{BeginStatus::Conflict, std::nullopt};
        }
        if (state.current && input.source_frame < state.current->source_frame) {
            return {BeginStatus::OutOfOrder, std::nullopt};
        }
        if (serial_ == std::numeric_limits<std::uint64_t>::max()) {
            return {BeginStatus::Invalid, std::nullopt};
        }
        const bool reset = input.camera_cut ||
            (state.current && input.resolutions != state.current->resolutions);
        state.previous = reset ? std::nullopt : state.current;
        state.previous_instances = reset ? InstanceMap{} : std::move(state.current_instances);
        state.current_instances.clear();
        state.current = input;
        state.serial = ++serial_;
        return {BeginStatus::Started, token(state)};
    }

    std::optional<FrameInput> current_frame(FrameToken frame) const
    {
        if (!valid(frame.view)) {
            return std::nullopt;
        }
        const auto& state = views_.at(static_cast<std::size_t>(frame.view));
        return state.current && token(state) == frame ? state.current : std::nullopt;
    }

    std::optional<CameraPair> cameras(FrameToken frame, ProjectionDomain domain) const
    {
        if (!valid(frame.view) || (domain != ProjectionDomain::World && domain != ProjectionDomain::FirstPerson)) {
            return std::nullopt;
        }
        const auto& state = views_.at(static_cast<std::size_t>(frame.view));
        if (!state.current || token(state) != frame) {
            return std::nullopt;
        }
        const auto current = camera(*state.current, domain);
        if (!current) {
            return std::nullopt;
        }
        const bool cut = domain == ProjectionDomain::FirstPerson && state.current->first_person_cut;
        const auto previous = state.previous && !cut ? camera(*state.previous, domain) : std::nullopt;
        return CameraPair{*current, previous, state.current->source_frame,
            previous ? std::optional{state.previous->source_frame} : std::nullopt};
    }

    CaptureResult capture(FrameToken frame, InstanceKey key, const InstanceSnapshot& snapshot)
    {
        if (!valid(key) || !valid(snapshot)) {
            return {CaptureStatus::Invalid, {}, {}};
        }
        if (retired(key)) {
            return {CaptureStatus::Retired, {}, {}};
        }
        const auto camera_pair = cameras(frame, snapshot.domain);
        if (!camera_pair) {
            return {current_frame(frame) ? CaptureStatus::MissingProjection : CaptureStatus::StaleFrame, {}, {}};
        }
        auto& state = views_.at(static_cast<std::size_t>(frame.view));
        const InstanceSlot slot{key, snapshot.domain};
        const auto old = state.previous_instances.find(slot);
        const auto previous = camera_pair->previous && old != state.previous_instances.end() &&
            old->second->domain == snapshot.domain && old->second->kind == snapshot.kind &&
            old->second->palette_layout == snapshot.palette_layout &&
            old->second->bones.size() == snapshot.bones.size() &&
            compatible_deformation(old->second->deformation, snapshot.deformation) ? old->second : nullptr;
        const auto found = state.current_instances.find(slot);
        if (found != state.current_instances.end()) {
            return *found->second == snapshot ? CaptureResult{CaptureStatus::Repeated, found->second, previous}
                : CaptureResult{CaptureStatus::Conflict, {}, {}};
        }
        auto current = std::make_shared<const InstanceSnapshot>(snapshot);
        state.current_instances.emplace(slot, current);
        return {CaptureStatus::Captured, std::move(current), previous};
    }

    bool reset_view(ViewId view)
    {
        if (!valid(view)) {
            return false;
        }
        views_.at(static_cast<std::size_t>(view)) = {};
        return true;
    }

    // Device/session-counter discontinuity. Object tombstones intentionally survive resets.
    void reset_all() { views_ = {}; }

    // Generation is monotonic per owner/visual pair. Retiring N also forbids older generations.
    bool retire(InstanceKey key)
    {
        if (!valid(key)) {
            return false;
        }
        auto& watermark = retired_generations_[{key.owner, key.visual}];
        watermark = std::max(watermark, key.generation);
        for (auto& view : views_) {
            const auto matches = [&](const auto& entry) {
                return entry.first.first.owner == key.owner && entry.first.first.visual == key.visual &&
                    entry.first.first.generation <= watermark;
            };
            std::erase_if(view.current_instances, matches);
            std::erase_if(view.previous_instances, matches);
        }
        return true;
    }

private:
    static std::uint64_t allocate_owner()
    {
        auto value = next_owner_.load(std::memory_order_relaxed);
        while (value != std::numeric_limits<std::uint64_t>::max()) {
            if (next_owner_.compare_exchange_weak(value, value + 1, std::memory_order_relaxed)) {
                return value;
            }
        }
        return 0; // Fail closed rather than wrap an owner identity.
    }
    bool retired(InstanceKey key) const
    {
        const auto found = retired_generations_.find({key.owner, key.visual});
        return found != retired_generations_.end() && key.generation <= found->second;
    }
    using InstanceSlot = std::pair<InstanceKey, ProjectionDomain>;
    using InstanceMap = std::map<InstanceSlot, std::shared_ptr<const InstanceSnapshot>>;
    static bool affine(const Mat4& matrix)
    {
        return finite(matrix) && std::abs(matrix.at(3, 0)) <= 1e-9 &&
            std::abs(matrix.at(3, 1)) <= 1e-9 && std::abs(matrix.at(3, 2)) <= 1e-9 &&
            std::abs(matrix.at(3, 3) - 1) <= 1e-9;
    }
    static bool valid(InstanceKey key)
    {
        return key.owner != 0 && key.visual != 0 && key.generation != 0;
    }
    static bool valid(const InstanceSnapshot& snapshot)
    {
        if (!affine(snapshot.world) ||
            (snapshot.domain != ProjectionDomain::World && snapshot.domain != ProjectionDomain::FirstPerson)) {
            return false;
        }
        const bool skinned = snapshot.kind == InstanceKind::Skinned;
        if (skinned) {
            if (snapshot.palette_layout == 0 || snapshot.bones.empty() || snapshot.bones.size() > 1024 ||
                !std::all_of(snapshot.bones.begin(), snapshot.bones.end(), affine)) {
                return false;
            }
        } else if (!snapshot.bones.empty() || snapshot.palette_layout != 0) {
            return false;
        }
        if (snapshot.deformation && (snapshot.deformation->layout == 0 ||
            snapshot.deformation->constants.empty() || snapshot.deformation->constants.size() > 256 ||
            !std::all_of(snapshot.deformation->constants.begin(), snapshot.deformation->constants.end(),
                [](Vec4 value) { return finite(value); }))) {
            return false;
        }
        switch (snapshot.kind) {
        case InstanceKind::Rigid:
            return !snapshot.deformation && !snapshot.detail;
        case InstanceKind::Skinned:
            return !snapshot.detail;
        case InstanceKind::Deformed:
            return snapshot.deformation.has_value() && !snapshot.detail;
        case InstanceKind::Detail:
            return snapshot.deformation && snapshot.detail && std::isfinite(snapshot.detail->fade_scale) &&
                snapshot.detail->fade_scale >= 0;
        default:
            return false;
        }
    }
    static bool compatible_deformation(const std::optional<DeformationState>& a,
        const std::optional<DeformationState>& b)
    {
        return a.has_value() == b.has_value() &&
            (!a || (a->layout == b->layout && a->constants.size() == b->constants.size()));
    }
    static bool valid(ViewId view) { return view == ViewId::Main || view == ViewId::Scope; }
    static bool valid(const CameraState& camera_state)
    {
        return affine(camera_state.view) && inverse(camera_state.view) &&
            inverse(camera_state.projection) && finite(camera_state.jitter_pixels);
    }
    static bool valid(const FrameInput& input)
    {
        return valid(input.view) && valid(input.world) && (!input.first_person || valid(*input.first_person)) &&
            plan_resolutions(input.resolutions.output, input.resolutions.render, input.resolutions.pda).has_value() &&
            jitter_projection(input.world.projection, input.world.jitter_pixels, input.resolutions.render) &&
            (!input.first_person || jitter_projection(input.first_person->projection,
                input.first_person->jitter_pixels, input.resolutions.render));
    }
    struct ViewState {
        std::optional<FrameInput> current;
        std::optional<FrameInput> previous;
        InstanceMap current_instances;
        InstanceMap previous_instances;
        std::uint64_t serial{};
    };
    static std::optional<CameraState> camera(const FrameInput& frame, ProjectionDomain domain)
    {
        return domain == ProjectionDomain::World ? std::optional{frame.world} : frame.first_person;
    }
    FrameToken token(const ViewState& state) const
    {
        return {state.current->view, state.current->source_frame, state.serial, owner_};
    }
    std::array<ViewState, 2> views_{};
    std::uint64_t serial_{};
    inline static std::atomic<std::uint64_t> next_owner_{1};
    const std::uint64_t owner_{allocate_owner()};
    std::map<std::pair<std::uint64_t, std::uint64_t>, std::uint64_t> retired_generations_;
};

} // namespace rsf::anomaly::temporal
