// SPDX-License-Identifier: MIT
#pragma once
#include "games/anomaly/temporal/fixture_platform.h"
#include "games/anomaly/temporal/frame_plan.h"
#include <memory>
#include <new>

namespace rsf::anomaly::temporal::fixture {
inline Mat4 decode(const rsf_anomaly_plan_matrix& matrix) {
    Mat4 result;
    std::copy_n(matrix.values, 16, result.values.begin());
    return result;
}
inline rsf_anomaly_plan_matrix encode(const Mat4& matrix) {
    rsf_anomaly_plan_matrix result{};
    std::copy_n(matrix.values.begin(), 16, result.values);
    return result;
}
inline Extent decode(rsf_anomaly_plan_extent size) { return {size.width, size.height}; }
inline rsf_anomaly_plan_extent encode(Extent size) { return {size.width, size.height}; }
inline Vec2 decode(rsf_anomaly_plan_vec2 v) { return {v.x, v.y}; }
inline Vec4 decode(rsf_anomaly_plan_vec4 v) { return {v.x, v.y, v.z, v.w}; }
inline CameraState decode(const rsf_anomaly_plan_camera& c) { return {decode(c.view), decode(c.projection), decode(c.jitter)}; }
inline rsf_anomaly_plan_camera encode(const CameraState& c) {
    return {encode(c.view), encode(c.projection), {c.jitter_pixels.x, c.jitter_pixels.y}};
}
inline FrameToken decode(const rsf_anomaly_plan_token& t) {
    return {static_cast<ViewId>(t.view), t.source_frame, t.serial, t.owner};
}
inline rsf_anomaly_plan_token encode(FrameToken t) {
    return {t.source_frame, t.serial, t.owner, static_cast<uint8_t>(t.view)};
}
inline InstanceKey decode(rsf_anomaly_plan_key key) { return {key.owner, key.visual, key.generation}; }
inline TemporalHistory& history(void* pointer) { return *static_cast<TemporalHistory*>(pointer); }

inline constexpr rsf_anomaly_plan_fixture oracle{
    .resolutions = [](const rsf_anomaly_plan_resolutions* in, rsf_anomaly_plan_resolutions* out) noexcept {
        const auto plan = plan_resolutions(decode(in->output), decode(in->render), decode(in->pda));
        if (plan) *out = {encode(plan->output), encode(plan->render), encode(plan->pda)};
        return plan.has_value();
    },
    .jitter = [](const rsf_anomaly_plan_matrix* source, rsf_anomaly_plan_vec2 jitter,
                 rsf_anomaly_plan_extent size, rsf_anomaly_plan_matrix* out) noexcept {
        const auto result = jitter_projection(decode(*source), decode(jitter), decode(size));
        if (result) *out = encode(*result);
        return result.has_value();
    },
    .multiply = [](const rsf_anomaly_plan_matrix* left, const rsf_anomaly_plan_matrix* right,
                   rsf_anomaly_plan_matrix* out) noexcept {
        const auto result = multiply(decode(*left), decode(*right));
        if (result) *out = encode(*result);
        return result.has_value();
    },
    .inverse = [](const rsf_anomaly_plan_matrix* source, rsf_anomaly_plan_matrix* out) noexcept {
        const auto result = inverse(decode(*source));
        if (result) *out = encode(*result);
        return result.has_value();
    },
    .transform = [](const rsf_anomaly_plan_matrix* m, rsf_anomaly_plan_vec4 point,
                    rsf_anomaly_plan_vec4* out) noexcept {
        const auto result = transform(decode(*m), decode(point));
        if (result) *out = {result->x, result->y, result->z, result->w};
        return result.has_value();
    },
    .pixel_motion = [](rsf_anomaly_plan_vec4 current, rsf_anomaly_plan_vec4 previous,
                        rsf_anomaly_plan_extent size, rsf_anomaly_plan_vec2 jitter,
                        rsf_anomaly_plan_vec2 previous_jitter, rsf_anomaly_plan_vec2* out) noexcept {
        const auto result = pixel_motion_from_jittered(decode(current), decode(previous), decode(size),
            decode(jitter), decode(previous_jitter));
        if (result) *out = {result->x, result->y};
        return result.has_value();
    },
    .matrix_read = [](const rsf_anomaly_plan_matrix* m, size_t row, size_t column, double* out) noexcept {
        try { const auto matrix = decode(*m); *out = matrix.at(row, column); return true; }
        catch (const std::out_of_range&) { return false; }
    },
    .matrix_write = [](rsf_anomaly_plan_matrix* m, size_t row, size_t column, double value) noexcept {
        try {
            auto matrix = decode(*m);
            matrix.at(row, column) = value;
            *m = encode(matrix);
            return true;
        } catch (const std::out_of_range&) { return false; }
    },
    .create = []() noexcept -> void* { return new (std::nothrow) TemporalHistory; },
    .destroy = [](void* h) noexcept { delete static_cast<TemporalHistory*>(h); },
    .begin = [](void* h, const rsf_anomaly_plan_frame* frame, rsf_anomaly_plan_token* token) noexcept -> uint32_t {
        try {
            const FrameInput input{static_cast<ViewId>(frame->view), frame->source_frame,
                {decode(frame->sizes.output), decode(frame->sizes.render), decode(frame->sizes.pda)},
                decode(frame->world),
                frame->has_first_person ? std::optional{decode(frame->first_person)} : std::nullopt,
                frame->camera_cut, frame->first_person_cut};
            const auto result = history(h).begin_frame(input);
            if (result.token) *token = encode(*result.token);
            return static_cast<uint32_t>(result.status);
        } catch (...) { return UINT32_MAX; }
    },
    .cameras = [](void* h, const rsf_anomaly_plan_token* token, uint8_t domain,
                   rsf_anomaly_plan_camera_pair* out) noexcept {
        const auto result = history(h).cameras(decode(*token), static_cast<ProjectionDomain>(domain));
        if (result) {
            out->current = encode(result->current);
            out->has_previous = result->previous.has_value();
            if (result->previous) out->previous = encode(*result->previous);
            out->previous_frame = result->previous_source_frame.value_or(0);
        }
        return result.has_value();
    },
    .plan = [](void* h, const rsf_anomaly_plan_token* token, rsf_anomaly_plan_view* out) noexcept {
        const auto result = plan_frame_view(history(h), decode(*token));
        if (result) {
            *out = {encode(result->frame),
                {encode(result->reconstructed_world), encode(result->world.viewport), encode(result->pda_content.viewport)},
                encode(result->screen_ui.viewport), encode(result->world.projection),
                result->first_person ? encode(result->first_person->projection) : rsf_anomaly_plan_matrix{},
                static_cast<uint32_t>(result->presentation), result->first_person.has_value()};
        }
        return result.has_value();
    },
    .capture = [](void* h, const rsf_anomaly_plan_token* token, rsf_anomaly_plan_key key,
                   const rsf_anomaly_plan_pose* pose) noexcept -> rsf_anomaly_plan_capture {
        try {
            InstanceSnapshot input;
            input.world = decode(pose->world);
            input.kind = static_cast<InstanceKind>(pose->kind);
            input.domain = static_cast<ProjectionDomain>(pose->domain);
            input.palette_layout = pose->palette_layout;
            for (size_t i = 0; i < pose->bone_count; ++i) input.bones.push_back(decode(pose->bones[i]));
            if (pose->has_deformation) {
                input.deformation.emplace();
                input.deformation->layout = pose->deformation_layout;
                for (size_t i = 0; i < pose->constant_count; ++i)
                    input.deformation->constants.push_back(decode(pose->constants[i]));
            }
            if (pose->has_detail) input.detail = DetailState{pose->fade};
            auto result = std::make_unique<CaptureResult>(history(h).capture(decode(*token), decode(key), input));
            const rsf_anomaly_plan_capture out{result.get(), result->current.get(), result->previous.get(),
                static_cast<uint32_t>(result->status)};
            static_cast<void>(result.release());
            return out;
        } catch (...) { return {nullptr, nullptr, nullptr, UINT32_MAX}; }
    },
    .drop_capture = [](void* result) noexcept { delete static_cast<CaptureResult*>(result); },
    .sample = [](const void* snapshot, rsf_anomaly_plan_sample* out) noexcept {
        const auto& sample = *static_cast<const InstanceSnapshot*>(snapshot);
        *out = {encode(sample.world), sample.bones.size() >= 2 ? encode(sample.bones[1]) : rsf_anomaly_plan_matrix{},
            sample.deformation && !sample.deformation->constants.empty() ? sample.deformation->constants[0].w : 0,
            sample.detail ? sample.detail->fade_scale : 0};
    },
    .retire = [](void* h, rsf_anomaly_plan_key key) noexcept {
        try { return history(h).retire(decode(key)); } catch (...) { return false; }
    },
    .reset_view = [](void* h, uint8_t view) noexcept { return history(h).reset_view(static_cast<ViewId>(view)); },
    .reset_all = [](void* h) noexcept { history(h).reset_all(); },
};
}
