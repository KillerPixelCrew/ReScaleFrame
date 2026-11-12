// SPDX-License-Identifier: GPL-3.0-only
#pragma once
// SR Stages 4 and 5: what the adapter hands the shared RSF SR pipeline at the S-B seam
// (docs/research/anomaly-sr-seam.md section 12). Pure rules, no engine memory and no D3D, so
// tests/anomaly_sr_pass.cpp checks every one of them without the game.
//
// Conventions. X-Ray Fmatrix bytes are row-major with ROW vectors (clip = v * mView * mProject), which is also the
// RSF camera convention (rsf_camera_frame, consumed by native_sr.cpp and the dense motion resolve as
// mul(float4, M) on row_major matrices). So engine matrices are copied, not transposed; only inverses and products
// are computed here, in double precision. Jitter is RSF render pixels (+X right, +Y down), the Stage 3 unit.
#include "sr_jitter.h"
#include "sr_settings.h"
#include <rescaleframe/game_renderer.h>
#include <rescaleframe/motion_decode.h>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstring>

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
inline constexpr float motion_to_uv[2]{1.0f, 1.0f};
// Written vectors are complete (they come from each draw's previous WVP, camera included), but coverage is sparse:
// zero asks the pipeline to reconstruct camera motion for the sentinel pixels (motion_resolve.h: written vectors
// replace the reconstruction, they are not added to it). tests/anomaly_sr_temporal_device.cpp checks this meaning.
inline constexpr uint32_t motion_camera_included = 0;
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
using MatrixD = std::array<double, 16>;
inline MatrixD widen(const Matrix& m) noexcept { MatrixD d{}; for (std::size_t i = 0; i < 16; ++i) d[i] = m[i]; return d; }
inline Matrix narrow(const MatrixD& m) noexcept { Matrix f{}; for (std::size_t i = 0; i < 16; ++i) f[i] = float(m[i]); return f; }
// Plain product: (a * b)[i][j] = sum_k a[i][k] b[k][j]. For row vectors v * a * b applies a first.
inline MatrixD multiply(const MatrixD& a, const MatrixD& b) noexcept {
    MatrixD m{};
    for (std::size_t i = 0; i < 4; ++i)
        for (std::size_t j = 0; j < 4; ++j) {
            double sum = 0;
            for (std::size_t k = 0; k < 4; ++k) sum += a[i * 4 + k] * b[k * 4 + j];
            m[i * 4 + j] = sum;
        }
    return m;
}
// Gauss-Jordan with partial pivoting. False for a singular or non-finite matrix.
inline bool invert(const MatrixD& m, MatrixD& out) noexcept {
    double a[4][8]{};
    for (int r = 0; r < 4; ++r) {
        for (int c = 0; c < 4; ++c) { a[r][c] = m[r * 4 + c]; if (!std::isfinite(a[r][c])) return false; }
        a[r][4 + r] = 1;
    }
    for (int c = 0; c < 4; ++c) {
        int pivot = c;
        for (int r = c + 1; r < 4; ++r) if (std::fabs(a[r][c]) > std::fabs(a[pivot][c])) pivot = r;
        if (std::fabs(a[pivot][c]) < 1e-300) return false;
        if (pivot != c) for (int k = 0; k < 8; ++k) std::swap(a[c][k], a[pivot][k]);
        const double inv = 1.0 / a[c][c];
        for (int k = 0; k < 8; ++k) a[c][k] *= inv;
        for (int r = 0; r < 4; ++r) {
            if (r == c) continue;
            const double f = a[r][c];
            if (f != 0) for (int k = 0; k < 8; ++k) a[r][k] -= f * a[c][k];
        }
    }
    for (int r = 0; r < 4; ++r) for (int c = 0; c < 4; ++c) out[r * 4 + c] = a[r][4 + c];
    return true;
}

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
    Planes out{};
    const double q = p[10], q43 = p[14], h = p[5];
    if (p[11] != 1.0f || p[15] != 0.0f || !(q > 1.0) || !(q43 < 0.0) || !(h > 0.0) || !std::isfinite(q) || !std::isfinite(q43))
        return out;
    const double n = -q43 / q, f = q43 / (1.0 - q);
    if (!(n > 0.0) || !(f > n) || !std::isfinite(f)) return out;
    out.near_plane = float(n);
    out.far_plane = float(f);
    out.vertical_fov = float(2.0 * std::atan(1.0 / h));
    out.valid = true;
    return out;
}
// Camera position (row 3) and forward (row 2) of view_to_world = inverse(view).
struct Pose { double position[3]{}, forward[3]{}; bool valid{}; };
inline Pose pose_of(const Matrix& view) noexcept {
    Pose pose{};
    MatrixD inverse{};
    if (!invert(widen(view), inverse)) return pose;
    for (int i = 0; i < 3; ++i) { pose.position[i] = inverse[12 + i]; pose.forward[i] = inverse[8 + i]; }
    const double length = std::sqrt(pose.forward[0] * pose.forward[0] + pose.forward[1] * pose.forward[1] + pose.forward[2] * pose.forward[2]);
    if (!(length > 1e-6)) return pose;
    for (double& value : pose.forward) value /= length;
    pose.valid = true;
    return pose;
}
// A camera cut: the main camera moved further than `distance` metres or turned by more than `degrees` between two
// consecutive main-view frames (teleports, level changes inside a session, cutscene cameras). History across it
// would ghost. A sprint is about 0.1 m per frame at 60 fps.
struct CutRule { float distance{4.0f}, degrees{45.0f}; };
inline bool camera_cut(const Pose& previous, const Pose& current, const CutRule& rule) noexcept {
    if (!previous.valid || !current.valid) return true;
    double d2 = 0, dot = 0;
    for (int i = 0; i < 3; ++i) {
        const double d = current.position[i] - previous.position[i];
        d2 += d * d;
        dot += current.forward[i] * previous.forward[i];
    }
    constexpr double pi = 3.14159265358979323846;
    return std::sqrt(d2) > rule.distance || dot < std::cos(double(rule.degrees) * pi / 180.0);
}

// ---- history breaks --------------------------------------------------------------------------------------------------
enum class ResetReason : uint8_t { None, First, Sequence, Cut, Settings, Device, Count };
inline const char* reset_name(ResetReason reason) noexcept {
    static constexpr const char* names[] = {"none", "first", "sequence", "cut", "settings", "device"};
    const auto i = static_cast<std::size_t>(reason);
    return i < std::size(names) ? names[i] : "unknown";
}
struct ResetFacts {
    bool first{};          // the first pass this pipeline session sees
    bool device{};         // a new device or pipeline session since the last pass
    bool settings{};       // backend, quality or preset changed since the last pass
    bool sequence{};       // the jitter sequence restarted: level load, menu, loading, vid_restart, scale or size change
    bool cut{};            // camera_cut()
};
// The first matching reason, in this order. The pipeline also restarts history by itself on a size, rectangle,
// session or non-consecutive native_frame change (native_sr.cpp); those are not repeated here.
inline ResetReason reset_reason(const ResetFacts& f) noexcept {
    if (f.first) return ResetReason::First;
    if (f.device) return ResetReason::Device;
    if (f.settings) return ResetReason::Settings;
    if (f.sequence) return ResetReason::Sequence;
    if (f.cut) return ResetReason::Cut;
    return ResetReason::None;
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
inline constexpr uint64_t main_view_key = 1;   // family, view, pass and history key of the one main view
inline PassError build_pass(const PassInputs& in, rsf_game_render_pass& pass) noexcept {
    std::memset(&pass, 0, sizeof(pass));
    if (!in.render_width || !in.render_height || in.render_width > in.output_width || in.render_height > in.output_height)
        return PassError::Sizes;
    if (!in.color || !in.color_output || !in.depth || !in.motion) return PassError::Resources;
    if (!in.current.valid) return PassError::Camera;
    // Stage 3's jitter must be in the projection this frame was rasterized with; without it there is nothing to
    // reconstruct from and the pass is refused, not submitted soft.
    if (!in.jitter.applied || in.jitter.scope || in.jitter.render_width != in.render_width || in.jitter.render_height != in.render_height ||
        !std::isfinite(in.jitter.current_x) || !std::isfinite(in.jitter.current_y))
        return PassError::Jitter;
    const auto planes = projection_planes(in.current.projection);
    if (!planes.valid) return PassError::Projection;
    const ViewCamera& prev = in.reset || !in.previous.valid ? in.current : in.previous;
    const MatrixD view = widen(in.current.view), projection = widen(in.current.projection);
    MatrixD clip_to_view{}, view_to_world{}, clip_to_world{};
    const MatrixD view_projection = multiply(view, projection);
    const MatrixD previous_view_projection = multiply(widen(prev.view), widen(prev.projection));
    if (!invert(projection, clip_to_view) || !invert(view, view_to_world) || !invert(view_projection, clip_to_world))
        return PassError::Camera;
    const MatrixD to_previous = multiply(clip_to_world, previous_view_projection);
    MatrixD from_previous{};
    if (!invert(to_previous, from_previous)) return PassError::Camera;

    pass.struct_size = sizeof(pass);
    pass.role = RSF_GAME_RENDER_SR;
    pass.session_id = in.session;
    pass.family_key = pass.view_key = pass.pass_key = main_view_key;
    pass.history_key = main_view_key;
    pass.native_frame = in.native_frame;
    pass.resource_generation = in.generation;
    pass.flags = RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG | (in.reset ? RSF_GAME_RENDER_RESET : 0u);
    pass.render_rect[2] = int32_t(in.render_width);
    pass.render_rect[3] = int32_t(in.render_height);
    pass.output_rect[2] = int32_t(in.output_width);
    pass.output_rect[3] = int32_t(in.output_height);
    pass.color_input = in.color;
    pass.color_output = in.color_output;
    pass.depth = in.depth;
    pass.motion = in.motion;
    pass.exposure = in.exposure;
    pass.motion_to_uv[0] = motion_to_uv[0];
    pass.motion_to_uv[1] = motion_to_uv[1];
    pass.motion_camera_included = motion_camera_included;
    auto& c = pass.camera;
    c.struct_size = sizeof(c);
    c.abi_version = RSF_GAME_FRAME_ABI_VERSION;
    const auto store = [](float (&target)[16], const MatrixD& m) { for (std::size_t i = 0; i < 16; ++i) target[i] = float(m[i]); };
    store(c.view_to_clip, projection);
    store(c.clip_to_view, clip_to_view);
    store(c.view_to_world, view_to_world);
    store(c.world_to_view, view);
    store(c.clip_to_previous_clip, to_previous);
    store(pass.previous_clip_to_clip, from_previous);
    c.jitter_pixels[0] = in.jitter.current_x;
    c.jitter_pixels[1] = in.jitter.current_y;
    c.previous_jitter_pixels[0] = in.jitter.previous_applied ? in.jitter.previous_x : 0.0f;
    c.previous_jitter_pixels[1] = in.jitter.previous_applied ? in.jitter.previous_y : 0.0f;
    c.render_width = in.render_width;
    c.render_height = in.render_height;
    c.output_width = in.output_width;
    c.output_height = in.output_height;
    c.near_plane = planes.near_plane;
    c.far_plane = planes.far_plane;
    c.vertical_fov_radians = planes.vertical_fov;
    c.depth_inverted = 0;       // standard depth from the depth twin (section 3, D1)
    c.motion_jittered = 0;      // the core PS removes both frames' jitter (PS b13)
    c.frame_time_seconds = in.frame_time_seconds;
    pass.camera_valid = 1;
    return PassError::None;
}
}
