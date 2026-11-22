// SPDX-License-Identifier: MIT
#pragma once
// SR Stages 4 and 5: what the adapter hands the shared RSF SR pipeline at the S-B seam
// (docs/research/anomaly-sr-seam.md section 12). Pure rules, no engine memory and no D3D, so
// the Rust pass fixtures exercise the record and camera math without the game.
//
// Conventions. X-Ray Fmatrix bytes are row-major with ROW vectors (clip = v * mView * mProject), which is also the
// RSF camera convention (rsf_camera_frame, consumed by native_sr.cpp and the dense motion resolve as
// mul(float4, M) on row_major matrices). So engine matrices are copied, not transposed; only inverses and products
// are computed in Rust, in double precision. Jitter is RSF render pixels (+X right, +Y down), the Stage 3 unit.
#include "sr_jitter.h"
#include "sr_stage_platform.h"
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/motion_decode.h>
#include <algorithm>
#include <array>
#include <bit>
#include <cstdint>

namespace rsf::anomaly::sr {
// ---- motion: the Anomaly motion target (RTV2, R16G16B16A16_FLOAT) -------------------------------------------------
// Channels (current-minus-previous UV x, UV y down, validity, 0) (shaders/core/README.md). A valid stationary pixel is
// (0, 0, 1, 0) and must stay zero motion; an unwritten or invalid pixel is (0, 0, 0, 0) and must be reconstructed
// from depth and the camera. So the decode marks unwritten pixels by the validity channel, never by a stored zero.
inline constexpr uint32_t motion_validity_channel = RSF_ANOMALY_SR_MOTION_VALIDITY_CHANNEL;
inline constexpr float motion_validity_threshold = RSF_ANOMALY_SR_MOTION_VALIDITY_THRESHOLD;
inline constexpr float motion_sentinel = RSF_ANOMALY_SR_MOTION_SENTINEL;   // outside any real UV motion, exact in half precision
extern "C" void rsf_anomaly_sr_motion_decode_params(rsf_motion_decode_params*) noexcept;
inline rsf_motion_decode_params motion_decode_params() noexcept {
    rsf_motion_decode_params p{};
    rsf_anomaly_sr_motion_decode_params(&p);
    return p;
}
// Decoded motion is already canonical current-minus-previous UV, Y down.
inline constexpr float motion_to_uv[2]{RSF_ANOMALY_SR_MOTION_TO_UV, RSF_ANOMALY_SR_MOTION_TO_UV};
// Written vectors are complete (they come from each draw's previous WVP, camera included), but coverage is sparse:
// zero asks the pipeline to reconstruct camera motion for the sentinel pixels (motion_resolve.h: written vectors
// replace the reconstruction, they are not added to it). tests/anomaly_sr_temporal_device.cpp checks this meaning.
inline constexpr uint32_t motion_camera_included = RSF_ANOMALY_SR_MOTION_CAMERA_INCLUDED;
// CPU reference of the decode the pipeline runs (motion_decode.cpp with the parameters above): the backend's
// input for one stored texel. Used by the contract test against the GPU fixture.
struct DecodedMotion { float x{}, y{}; bool unwritten{}; };
inline DecodedMotion decode_reference(const float stored[4]) noexcept {
    const auto p = motion_decode_params();
    if (!(stored[p.validity_channel] > p.validity_threshold)) return {p.invalid_value, p.invalid_value, true};
    return {(stored[0] - p.bias_x) * p.scale_x * p.output_scale_x, (stored[1] - p.bias_y) * p.scale_y * p.output_scale_y, false};
}
// First person: unwritten pixels inside the rmNear depth range [0, 0.02] belong to HUD geometry rendered with its own
// projection; reconstructing them with the world camera would give the weapon the camera's rotation. They get zero
// motion instead (section 3; a heuristic, UNVERIFIED in game). World geometry reaches d < 0.02 only within 2% of
// the near plane.
inline constexpr float first_person_depth_limit = 0.02f;

// ---- matrices ------------------------------------------------------------------------------------------------------
using Matrix = std::array<float, 16>;
// ---- the main-view camera of one frame ------------------------------------------------------------------------------
// Device.mView and the UNJITTERED Device.mProject, read at H1 before Stage 3 jitters them.
struct ViewCamera {
    Matrix view{}, projection{};
    bool valid{};
};
// ---- history breaks --------------------------------------------------------------------------------------------------
enum class ResetReason : uint8_t { None, First, Sequence, Cut, Settings, Device, Count };
struct ResetFacts {
    bool first{}, device{}, settings{}, sequence{}, cut{};
};
inline rsf_anomaly_sr_reset_facts reset_facts(const ResetFacts& facts) noexcept {
    return {facts.first, facts.device, facts.settings, facts.sequence, facts.cut};
}
// ---- the pass ----------------------------------------------------------------------------------------------------
struct PassInputs {
    uint64_t session{};                 // adapter SR session (QPC at install)
    uint64_t native_frame{};            // JitterState::frame: main-view frames, consecutive across scope frames
    uint32_t generation{};              // twin generation
    uint32_t render_width{}, render_height{}, output_width{}, output_height{};
    ViewCamera current{}, previous{};   // previous: the last main-view frame's camera (current again on a reset)
    JitterState jitter{};
    float frame_time_seconds{};
    bool reset{};
    void* color{};                      // render-size R16G16B16A16_FLOAT, f^-1 of the generic0 twin (exposure-scaled linear)
    void* color_output{};               // output-size texture in the pipeline's output format
    void* depth{};                      // the R24G8_TYPELESS depth twin (device depth, standard, finite far)
    void* motion{};                     // render-size R16G16B16A16_FLOAT prepared motion (validity in channel 2)
    bool motion_missing{};              // no motion target this frame: `motion` holds no written vector (camera-only)
    void* exposure{};                   // 1x1 R32_FLOAT, or null for backend auto exposure
};
enum class PassError : uint8_t { None, Sizes, Camera, Projection, Jitter, Resources };
inline rsf_anomaly_sr_pass_inputs pass_inputs(const PassInputs& in) noexcept {
    const auto camera = [](const ViewCamera& input) {
        rsf_anomaly_sr_camera out{};
        std::copy(input.view.begin(), input.view.end(), out.view);
        std::copy(input.projection.begin(), input.projection.end(), out.projection);
        out.valid = input.valid;
        return out;
    };
    return {
        {in.native_frame, in.generation, in.render_width, in.render_height, in.output_width, in.output_height,
            camera(in.current), camera(in.previous), std::bit_cast<rsf_anomaly_sr_jitter_state>(in.jitter),
            in.frame_time_seconds, in.motion_missing, false, false, false},
        in.session, in.reset, in.color, in.color_output, in.depth, in.motion, in.exposure
    };
}

inline PassError build_pass(const PassInputs& in, rsf_game_render_pass& pass) noexcept {
    const auto input = pass_inputs(in);
    return static_cast<PassError>(rsf_anomaly_sr_build_pass(&input, &pass));
}
}
