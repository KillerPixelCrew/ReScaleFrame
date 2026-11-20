// SPDX-License-Identifier: MIT
#pragma once
// SR Stages 4 and 5: what the adapter hands the shared RSF SR pipeline at the S-B seam
// (docs/research/anomaly-sr-seam.md section 12). Pure rules, no engine memory and no D3D, so
// tests/anomaly_sr_pass.cpp checks every one of them without the game.
//
// Conventions. X-Ray Fmatrix bytes are row-major with ROW vectors (clip = v * mView * mProject), which is also the
// RSF camera convention (rsf_camera_frame, consumed by native_sr.cpp and the dense motion resolve as
// mul(float4, M) on row_major matrices). So engine matrices are copied, not transposed; only inverses and products
// are computed in Rust, in double precision. Jitter is RSF render pixels (+X right, +Y down), the Stage 3 unit.
#include "sr_jitter.h"
#include "sr_settings.h"
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/motion_decode.h>
#include <algorithm>
#include <array>
#include <bit>
#include <cstdint>

namespace rsf::anomaly::sr {
// ---- engine identity: adapter-supplied metadata for Streamline/NGX, never a runtime switch ------------------------
inline constexpr char engine_version[] = "X-Ray Monolith 1.5.3 (S.T.A.L.K.E.R. Anomaly, DX11)";
inline constexpr char project_id[] = "8d3b6f52-1c4e-4f7a-b0d9-2e6a51c7f3a4";
inline constexpr float view_space_to_meters = 1.0f;   // X-Ray world units are metres

// ---- shared settings values: sr_settings.h -----------------------------------------------------------------------

// ---- motion: the Anomaly motion target (RTV2, R16G16B16A16_FLOAT) -------------------------------------------------
// Channels (current-minus-previous UV x, UV y down, validity, 0) (shaders/core/README.md). A valid stationary pixel is
// (0, 0, 1, 0) and must stay zero motion; an unwritten or invalid pixel is (0, 0, 0, 0) and must be reconstructed
// from depth and the camera. So the decode marks unwritten pixels by the validity channel, never by a stored zero.
inline constexpr uint32_t motion_validity_channel = 2;
inline constexpr float motion_validity_threshold = 0.5f;
inline constexpr float motion_sentinel = -1000.0f;   // outside any real UV motion, exact in half precision
inline rsf_motion_decode_params motion_decode_params() noexcept {
    rsf_motion_decode_params p{};
    p.struct_size = sizeof(p);
    p.scale_x = p.scale_y = 1.0f;          // stored values are already UV
    p.bias_x = p.bias_y = 0.0f;
    p.output_scale_x = p.output_scale_y = 1.0f;
    p.invalid_value = motion_sentinel;
    p.zero_means_unwritten = 0;
    p.validity_enabled = 1;
    p.validity_channel = motion_validity_channel;
    p.validity_threshold = motion_validity_threshold;
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
// build_projection_HAT (xrCore/_matrix.h:632-659): _33 = Q = f/(f-n), _43 = -Q*n, _34 = 1, _44 = 0; standard
// (not inverted) depth d = Q + _43 / z. Element (row r, column c) is index r*4+c.
struct Planes { float near_plane{}, far_plane{}, vertical_fov{}; bool valid{}; };
inline Planes projection_planes(const Matrix& p) noexcept {
    float projection[16]{};
    std::copy(p.begin(), p.end(), projection);
    rsf_anomaly_sr_planes out{};
    rsf_anomaly_sr_projection_planes(&projection, &out);
    return {out.near_plane, out.far_plane, out.vertical_fov, out.valid};
}

// Camera position (row 3) and forward (row 2) of view_to_world = inverse(view).
struct Pose { double position[3]{}, forward[3]{}; bool valid{}; };
inline Pose pose_of(const Matrix& v) noexcept {
    float view[16]{};
    std::copy(v.begin(), v.end(), view);
    rsf_anomaly_sr_pose out{};
    rsf_anomaly_sr_pose_of(&view, &out);
    return {{out.position[0], out.position[1], out.position[2]},
        {out.forward[0], out.forward[1], out.forward[2]}, out.valid};
}

// Camera displacement in metres and change in forward direction in degrees.
struct CutRule { float distance{4.0f}, degrees{45.0f}; };
inline bool camera_cut(const Pose& previous, const Pose& current, const CutRule& rule) noexcept {
    const auto to_c = [](const Pose& p) {
        return rsf_anomaly_sr_pose{{p.position[0], p.position[1], p.position[2]},
            {p.forward[0], p.forward[1], p.forward[2]}, p.valid};
    };
    const auto a = to_c(previous), b = to_c(current);
    return rsf_anomaly_sr_camera_cut(&a, &b, rule.distance, rule.degrees);
}

// ---- history breaks --------------------------------------------------------------------------------------------------
enum class ResetReason : uint8_t { None, First, Sequence, Cut, Settings, Device, Count };
inline const char* reset_name(ResetReason reason) noexcept {
    static constexpr const char* names[] = {"none", "first", "sequence", "cut", "settings", "device"};
    const auto i = static_cast<std::size_t>(reason);
    return i < std::size(names) ? names[i] : "unknown";
}
struct ResetFacts {
    bool first{}, device{}, settings{}, sequence{}, cut{};
};
inline rsf_anomaly_sr_reset_facts reset_facts(const ResetFacts& facts) noexcept {
    return {facts.first, facts.device, facts.settings, facts.sequence, facts.cut};
}
// The pipeline separately resets on rectangle, session or non-consecutive native_frame changes.
inline ResetReason reset_reason(const ResetFacts& facts) noexcept {
    const auto input = reset_facts(facts);
    return static_cast<ResetReason>(rsf_anomaly_sr_reset_reason(&input));
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
inline const char* pass_error_name(PassError e) noexcept {
    static constexpr const char* names[] = {"none", "sizes", "camera", "projection", "jitter", "resources"};
    const auto i = static_cast<std::size_t>(e);
    return i < std::size(names) ? names[i] : "unknown";
}
inline constexpr uint64_t main_view_key = RSF_ANOMALY_SR_MAIN_VIEW_KEY;

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
