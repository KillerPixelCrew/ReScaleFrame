// SPDX-License-Identifier: MIT
#pragma once
#include "../games/anomaly/temporal/fixture_platform.h"
#include "temporal_types.h"
#include <algorithm>
#include <optional>
#include <vector>

namespace rsf::anomaly::temporal {
using ResolutionPlan = rsf_anomaly_plan_resolutions;

// Only the fixed camera and snapshot representations cross to the Rust fixture.
struct Camera {
    Mat4 view, projection;
    rsf_anomaly_plan_vec2 jitter{};
};
struct FrameInput {
    ViewId view{};
    uint64_t source_frame{};
    ResolutionPlan resolutions{};
    Camera world{};
};
struct FrameToken {
    ViewId view;
    rsf_anomaly_plan_token native;
};
enum class BeginStatus : uint32_t { Started, Repeated, Invalid, OutOfOrder, Conflict };
struct BeginResult {
    BeginStatus status;
    std::optional<FrameToken> token;
};
enum class InstanceKind : uint8_t { Rigid, Skinned };
struct InstanceSnapshot {
    Mat4 world{};
    ProjectionDomain domain{};
    InstanceKind kind{};
    uint64_t palette_layout{};
    std::vector<Mat4> bones;
};
enum class CaptureStatus : uint32_t { Captured, Repeated, Invalid, StaleFrame, Conflict, Retired, MissingProjection };
struct PreviousWorld { Mat4 world; };
struct CaptureResult {
    CaptureStatus status{CaptureStatus::Invalid};
    std::optional<PreviousWorld> previous;
};

inline rsf_anomaly_plan_matrix matrix(const Mat4& value) {
    rsf_anomaly_plan_matrix out{};
    std::copy(value.values.begin(), value.values.end(), out.values);
    return out;
}
inline rsf_anomaly_plan_key history_key(InstanceKey key) { return {key.owner, key.visual, key.generation}; }

class TemporalHistory {
public:
    TemporalHistory() : owner_(rsf_anomaly_plan_history_create()) {}
    ~TemporalHistory() { rsf_anomaly_plan_history_destroy(owner_); }
    TemporalHistory(const TemporalHistory&) = delete;
    TemporalHistory& operator=(const TemporalHistory&) = delete;

    BeginResult begin_frame(const FrameInput& frame) {
        rsf_anomaly_plan_frame input{};
        input.view = static_cast<uint8_t>(frame.view);
        input.source_frame = frame.source_frame;
        input.sizes = frame.resolutions;
        input.world = {matrix(frame.world.view), matrix(frame.world.projection), frame.world.jitter};
        rsf_anomaly_plan_token token{};
        const auto status = static_cast<BeginStatus>(rsf_anomaly_plan_history_begin(owner_, &input, &token));
        if (token.owner == 0) return {status, std::nullopt};
        return {status, FrameToken{static_cast<ViewId>(token.view), token}};
    }

    CaptureResult capture(FrameToken token, InstanceKey key, const InstanceSnapshot& snapshot) {
        std::vector<rsf_anomaly_plan_matrix> bones;
        bones.reserve(snapshot.bones.size());
        for (const auto& bone : snapshot.bones) bones.push_back(matrix(bone));
        const rsf_anomaly_plan_pose input{matrix(snapshot.world), snapshot.palette_layout, bones.data(), bones.size(),
            static_cast<uint8_t>(snapshot.domain), static_cast<uint8_t>(snapshot.kind)};
        const auto identity = history_key(key);
        rsf_anomaly_plan_capture out{};
        rsf_anomaly_plan_history_capture(owner_, &token.native, &identity, &input, &out);
        CaptureResult result{static_cast<CaptureStatus>(out.status), std::nullopt};
        if (out.has_previous) {
            auto& value = result.previous.emplace().world.values;
            std::copy_n(out.previous_world.values, value.size(), value.begin());
        }
        return result;
    }

    bool retire(InstanceKey key) {
        const auto identity = history_key(key);
        return rsf_anomaly_plan_history_retire(owner_, &identity);
    }

private:
    void* owner_;
};
}
