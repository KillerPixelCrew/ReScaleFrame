// SPDX-License-Identifier: GPL-3.0-only
// SR Stages 4 and 5 pass construction (games/anomaly/src/sr_pass.h, sr_plan.h) without the game or a GPU: the motion
// decode parameters and their CPU reference on (0,0,1,0) versus (0,0,0,0), the X-Ray projection planes, the camera
// matrices handed to the pipeline (clip-to-previous reprojects a world point exactly), jitter units and depth
// convention, rectangles and flags, every build_pass refusal, the reset reasons and their order, the camera-cut rule,
// the settings the adapter can apply and the render size it takes from a backend's plan.
#include "sr_pass.h"
#include "sr_plan.h"

#include <cmath>
#include <cstdio>
#include <limits>
#include <string_view>

namespace {
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)
bool near(double a, double b, double tolerance) { return std::fabs(a - b) <= tolerance; }

// Fmatrix::build_projection_HAT for (fov, aspect, near, far): row vectors, standard depth.
Matrix projection(float fov = 1.2f, float aspect = 16.0f / 9.0f, float n = 0.2f, float f = 900.0f) {
    const float h = 1.0f / std::tan(fov * 0.5f), w = h / aspect, q = f / (f - n);
    return {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * n, 0};
}
// A view matrix (world to view, row vectors) for a camera at `p` turned by `yaw` about +Y.
Matrix view_at(double px, double py, double pz, double yaw) {
    const double c = std::cos(yaw), s = std::sin(yaw);
    // camera basis (rows of view_to_world): right, up, forward, position
    const double right[3]{c, 0, -s}, up[3]{0, 1, 0}, forward[3]{s, 0, c}, pos[3]{px, py, pz};
    Matrix v{};
    for (int i = 0; i < 3; ++i) { v[i * 4 + 0] = float(right[i]); v[i * 4 + 1] = float(up[i]); v[i * 4 + 2] = float(forward[i]); }
    v[12] = float(-(pos[0] * right[0] + pos[1] * right[1] + pos[2] * right[2]));
    v[13] = float(-(pos[0] * up[0] + pos[1] * up[1] + pos[2] * up[2]));
    v[14] = float(-(pos[0] * forward[0] + pos[1] * forward[1] + pos[2] * forward[2]));
    v[15] = 1;
    return v;
}
std::array<double, 4> row_times(const std::array<double, 4>& v, const float* m) {
    std::array<double, 4> out{};
    for (int c = 0; c < 4; ++c) for (int r = 0; r < 4; ++r) out[c] += v[r] * double(m[r * 4 + c]);
    return out;
}
JitterState applied_jitter(uint32_t w, uint32_t h, float x = 0.25f, float y = -0.125f) {
    JitterState s{};
    s.frame = 7; s.applied = true; s.current_x = x; s.current_y = y; s.render_width = w; s.render_height = h;
    s.previous_applied = true; s.previous_x = -0.375f; s.previous_y = 0.0625f; s.previous_render_width = w; s.previous_render_height = h;
    return s;
}
int dummy[4];
PassInputs inputs() {
    PassInputs in{};
    in.session = 0x1234; in.native_frame = 7; in.generation = 3;
    in.render_width = 1280; in.render_height = 720; in.output_width = 1920; in.output_height = 1080;
    in.current = {view_at(10, 2, -5, 0.3), projection(), true};
    in.previous = {view_at(10.1, 2, -5, 0.31), projection(), true};
    in.jitter = applied_jitter(1280, 720);
    in.frame_time_seconds = 1.0f / 60;
    in.color = &dummy[0]; in.color_output = &dummy[1]; in.depth = &dummy[2]; in.motion = &dummy[3];
    return in;
}

void motion_decode() {
    const auto p = motion_decode_params();
    CHECK(p.struct_size == sizeof(rsf_motion_decode_params) && p.struct_size > RSF_MOTION_DECODE_PARAMS_SIZE_V1);
    CHECK(p.scale_x == 1 && p.scale_y == 1 && p.bias_x == 0 && p.bias_y == 0 && p.output_scale_x == 1 && p.output_scale_y == 1);
    CHECK(p.validity_enabled == 1 && p.validity_channel == 2 && p.validity_threshold == 0.5f && p.zero_means_unwritten == 0);
    CHECK(p.invalid_value == -1000.0f);
    // A valid stationary vector stays zero motion; the same zero without validity is unwritten.
    const float valid_zero[4]{0, 0, 1, 0}, unwritten[4]{0, 0, 0, 0}, valid_motion[4]{0.0125f, -0.25f, 1, 0}, invalid_motion[4]{0.5f, 0.5f, 0, 0};
    const float half_validity[4]{0.1f, 0.1f, 0.5f, 0};   // exactly at the threshold: not valid (strictly greater)
    auto d = decode_reference(valid_zero);
    CHECK(!d.unwritten && d.x == 0 && d.y == 0);
    d = decode_reference(unwritten);
    CHECK(d.unwritten && d.x == -1000.0f && d.y == -1000.0f);
    d = decode_reference(valid_motion);
    CHECK(!d.unwritten && d.x == 0.0125f && d.y == -0.25f);
    d = decode_reference(invalid_motion);
    CHECK(d.unwritten && d.x == -1000.0f);
    CHECK(decode_reference(half_validity).unwritten);
    // Canonical UV and sparse coverage: the pipeline reconstructs camera motion for the sentinel pixels.
    CHECK(motion_to_uv[0] == 1.0f && motion_to_uv[1] == 1.0f && motion_camera_included == 0);
    CHECK(first_person_depth_limit == 0.02f);
}

void planes() {
    const auto p = projection(1.2f, 16.0f / 9.0f, 0.2f, 900.0f);
    const auto planes = projection_planes(p);
    CHECK(planes.valid && near(planes.near_plane, 0.2, 1e-5) && near(planes.far_plane, 900.0, 0.5) && near(planes.vertical_fov, 1.2, 1e-5));
    // The first-person projection (R_VIEWPORT_NEAR 0.005) is also a valid perspective.
    CHECK(projection_planes(projection(0.9f, 1.5f, 0.005f, 100.0f)).valid);
    auto bad = p; bad[11] = 0; bad[15] = 1;   // orthographic
    CHECK(!projection_planes(bad).valid);
    bad = p; bad[14] = 0.1f;                  // inverted depth (_43 > 0): not this engine's convention
    CHECK(!projection_planes(bad).valid);
    bad = p; bad[10] = std::numeric_limits<float>::quiet_NaN();
    CHECK(!projection_planes(bad).valid);
}

void camera_matrices() {
    auto in = inputs();
    rsf_game_render_pass pass;
    CHECK(build_pass(in, pass) == PassError::None);
    const auto& c = pass.camera;
    CHECK(pass.struct_size == sizeof(pass) && c.struct_size == sizeof(c) && c.abi_version == RSF_GAME_FRAME_ABI_VERSION);
    // view_to_clip is the unjittered engine projection, copied (row vectors in both conventions).
    for (int i = 0; i < 16; ++i) CHECK(c.view_to_clip[i] == in.current.projection[i] && c.world_to_view[i] == in.current.view[i]);
    // view_to_world rows: right, up, forward, position (native_sr.cpp reads position from +12, right +0, up +4, forward +8).
    CHECK(near(c.view_to_world[12], 10, 1e-4) && near(c.view_to_world[13], 2, 1e-4) && near(c.view_to_world[14], -5, 1e-4));
    CHECK(near(c.view_to_world[8], std::sin(0.3), 1e-6) && near(c.view_to_world[10], std::cos(0.3), 1e-6) && near(c.view_to_world[5], 1, 1e-6));
    // A world point projected by the current camera, carried through clip_to_previous_clip, lands where the previous
    // camera projects it; previous_clip_to_clip is the inverse.
    const std::array<double, 4> world{14.0, 3.0, 20.0, 1.0};
    const auto clip = row_times(row_times(world, in.current.view.data()), in.current.projection.data());
    const auto expected_previous = row_times(row_times(world, in.previous.view.data()), in.previous.projection.data());
    const auto previous = row_times(clip, c.clip_to_previous_clip);
    for (int i = 0; i < 2; ++i) CHECK(near(previous[i] / previous[3], expected_previous[i] / expected_previous[3], 1e-4));
    CHECK(near(previous[2] / previous[3], expected_previous[2] / expected_previous[3], 1e-5));
    const auto back = row_times(previous, pass.previous_clip_to_clip);
    for (int i = 0; i < 3; ++i) CHECK(near(back[i] / back[3], clip[i] / clip[3], 1e-4));
    // clip_to_view inverts view_to_clip.
    const auto view_point = row_times(clip, c.clip_to_view);
    const auto direct = row_times(world, in.current.view.data());
    for (int i = 0; i < 3; ++i) CHECK(near(view_point[i] / view_point[3], direct[i], 1e-3));
    // Planes, FOV, depth convention, jitter units and the motion contract.
    CHECK(near(c.near_plane, 0.2, 1e-5) && near(c.far_plane, 900, 0.5) && near(c.vertical_fov_radians, 1.2, 1e-5));
    CHECK(c.depth_inverted == 0 && c.motion_jittered == 0);
    CHECK(c.jitter_pixels[0] == 0.25f && c.jitter_pixels[1] == -0.125f);            // render pixels, +X right, +Y down, unchanged
    CHECK(c.previous_jitter_pixels[0] == -0.375f && c.previous_jitter_pixels[1] == 0.0625f);
    CHECK(c.render_width == 1280 && c.render_height == 720 && c.output_width == 1920 && c.output_height == 1080);
    CHECK(near(c.frame_time_seconds, 1.0 / 60, 1e-7));
    CHECK(pass.motion_to_uv[0] == 1 && pass.motion_to_uv[1] == 1 && pass.motion_camera_included == 0 && pass.camera_valid == 1);
    // A reset uses the current camera as the previous one: identity reprojection.
    in.reset = true;
    CHECK(build_pass(in, pass) == PassError::None);
    for (int r = 0; r < 4; ++r)
        for (int col = 0; col < 4; ++col) CHECK(near(pass.camera.clip_to_previous_clip[r * 4 + col], r == col ? 1.0 : 0.0, 1e-4));
    // No previous camera behaves the same way.
    in.reset = false; in.previous.valid = false;
    CHECK(build_pass(in, pass) == PassError::None && near(pass.camera.clip_to_previous_clip[0], 1, 1e-4));
    // A previous frame without jitter reports zero previous jitter.
    in = inputs(); in.jitter.previous_applied = false;
    CHECK(build_pass(in, pass) == PassError::None && pass.camera.previous_jitter_pixels[0] == 0 && pass.camera.previous_jitter_pixels[1] == 0);
}

void rects_and_flags() {
    auto in = inputs();
    rsf_game_render_pass pass;
    CHECK(build_pass(in, pass) == PassError::None);
    CHECK(pass.role == RSF_GAME_RENDER_SR && pass.session_id == 0x1234 && pass.native_frame == 7 && pass.resource_generation == 3);
    CHECK(pass.history_key == main_view_key && pass.family_key == main_view_key && pass.view_key == main_view_key);
    CHECK(pass.render_rect[0] == 0 && pass.render_rect[1] == 0 && pass.render_rect[2] == 1280 && pass.render_rect[3] == 720);
    CHECK(pass.output_rect[0] == 0 && pass.output_rect[1] == 0 && pass.output_rect[2] == 1920 && pass.output_rect[3] == 1080);
    CHECK(pass.flags == (RSF_GAME_RENDER_PRIMARY | RSF_GAME_RENDER_NO_FG));
    CHECK(pass.color_input == &dummy[0] && pass.color_output == &dummy[1] && pass.depth == &dummy[2] && pass.motion == &dummy[3]);
    CHECK(pass.exposure == nullptr && pass.ui_input == nullptr && pass.swapchain == nullptr);
    in.reset = true;
    CHECK(build_pass(in, pass) == PassError::None && (pass.flags & RSF_GAME_RENDER_RESET));
    in.exposure = &dummy[0];
    CHECK(build_pass(in, pass) == PassError::None && pass.exposure == &dummy[0]);
    // Native (DLAA) quality: render == output.
    in = inputs(); in.render_width = 1920; in.render_height = 1080; in.jitter = applied_jitter(1920, 1080);
    CHECK(build_pass(in, pass) == PassError::None && pass.render_rect[2] == 1920);
}

void refusals() {
    rsf_game_render_pass pass;
    auto in = inputs(); in.render_width = 2000;
    CHECK(build_pass(in, pass) == PassError::Sizes);
    in = inputs(); in.render_height = 0;
    CHECK(build_pass(in, pass) == PassError::Sizes);
    for (int i = 0; i < 4; ++i) {
        in = inputs();
        void** slot[] = {&in.color, &in.color_output, &in.depth, &in.motion};
        *slot[i] = nullptr;
        CHECK(build_pass(in, pass) == PassError::Resources);
    }
    in = inputs(); in.current.valid = false;
    CHECK(build_pass(in, pass) == PassError::Camera);
    // Jitter: not applied (no rmNear slot, SRJitter=0), a scope frame, a stale render size, a non-finite sample.
    in = inputs(); in.jitter.applied = false;
    CHECK(build_pass(in, pass) == PassError::Jitter);
    in = inputs(); in.jitter.scope = true;
    CHECK(build_pass(in, pass) == PassError::Jitter);
    in = inputs(); in.jitter.render_width = 1286;
    CHECK(build_pass(in, pass) == PassError::Jitter);
    in = inputs(); in.jitter.current_x = std::numeric_limits<float>::infinity();
    CHECK(build_pass(in, pass) == PassError::Jitter);
    in = inputs(); in.current.projection[11] = 0;
    CHECK(build_pass(in, pass) == PassError::Projection);
    in = inputs(); in.current.view = {};   // singular
    CHECK(build_pass(in, pass) == PassError::Camera);
    // A refusal leaves nothing a caller could submit.
    CHECK(pass.struct_size == 0 && pass.role == 0);
}

void resets() {
    CHECK(reset_reason({}) == ResetReason::None);
    CHECK(reset_reason({true, true, true, true, true}) == ResetReason::First);
    CHECK(reset_reason({false, true, true, true, true}) == ResetReason::Device);
    CHECK(reset_reason({false, false, true, true, true}) == ResetReason::Settings);
    CHECK(reset_reason({false, false, false, true, true}) == ResetReason::Sequence);
    CHECK(reset_reason({false, false, false, false, true}) == ResetReason::Cut);
    CHECK(std::string_view{reset_name(ResetReason::Cut)} == "cut" && std::string_view{reset_name(ResetReason::Sequence)} == "sequence");
    // Camera cuts: small motion is not a cut; a teleport or a large turn is; an unknown pose is.
    const CutRule rule{};
    const auto a = pose_of(view_at(0, 0, 0, 0));
    CHECK(a.valid && near(a.forward[2], 1, 1e-6));
    CHECK(!camera_cut(a, pose_of(view_at(0.1, 0, 0.05, 0.02)), rule));     // a sprint step and a small turn
    CHECK(!camera_cut(a, pose_of(view_at(3.9, 0, 0, 0)), rule));
    CHECK(camera_cut(a, pose_of(view_at(4.1, 0, 0, 0)), rule));            // teleport
    CHECK(!camera_cut(a, pose_of(view_at(0, 0, 0, 0.7)), rule));            // 40 degrees
    CHECK(camera_cut(a, pose_of(view_at(0, 0, 0, 0.9)), rule));             // 52 degrees
    CHECK(camera_cut(Pose{}, a, rule) && camera_cut(a, Pose{}, rule));
    CHECK(!camera_cut(a, pose_of(view_at(8, 0, 0, 0)), CutRule{10.0f, 45.0f}));
}

void settings_and_plans() {
    CHECK(adapter_backend(backend_dlss) && adapter_backend(backend_fsr3) && adapter_backend(backend_fsr4) && adapter_backend(backend_xess));
    CHECK(!adapter_backend(0) && !adapter_backend(2) && !adapter_backend(6) && !adapter_backend(7) && !adapter_backend(40));
    for (uint32_t q = 0; q < 4; ++q) CHECK(adapter_quality(q));
    CHECK(!adapter_quality(4) && !adapter_quality(5));                      // ultra performance is below the half-size floor
    const SrSettings current{true, backend_xess, 2, 0};
    auto s = sanitize({false, 2, 4, 3}, current);                           // FSR2 and ultra performance refused, field by field
    CHECK(!s.enabled && s.backend == backend_xess && s.quality == 2 && s.preset == 3);
    s = sanitize({true, backend_fsr3, 0, 0}, current);
    CHECK(s.backend == backend_fsr3 && s.quality == 0);
    // Render size from a backend's plan: kept when inside [ceil(output/2), output], refused otherwise.
    ScalePlan plan{};
    CHECK(make_plan_for_render(1920, 1080, 1280, 720, plan) && plan.render_width == 1280 && plan.render_height == 720 &&
        near(plan.scale, 2.0 / 3, 1e-6));
    CHECK(make_plan_for_render(1920, 1080, 960, 540, plan) && plan.render_width == 960);           // performance, exactly half
    CHECK(make_plan_for_render(1920, 1080, 1920, 1080, plan) && plan.identity());                   // native
    CHECK(!make_plan_for_render(1920, 1080, 640, 360, plan) && plan.render_width == 0);            // ultra performance
    CHECK(!make_plan_for_render(1920, 1080, 2000, 1080, plan) && !make_plan_for_render(0, 1080, 960, 540, plan));
    CHECK(!make_plan_for_render(1921, 1081, 960, 540, plan) && make_plan_for_render(1921, 1081, 961, 541, plan));
    // Identity metadata.
    CHECK(view_space_to_meters == 1.0f && std::string_view{project_id}.size() == 36);
}
}

int main(int argc, char** argv) {
    struct { const char* name; void (*run)(); } cases[] = {{"motion_decode", motion_decode}, {"planes", planes},
        {"camera", camera_matrices}, {"rects", rects_and_flags}, {"refusals", refusals}, {"resets", resets},
        {"settings", settings_and_plans}};
    int ran = 0;
    for (const auto& item : cases) {
        if (argc > 1 && std::string_view{argv[1]} != item.name) continue;
        item.run();
        ++ran;
    }
    if (!ran) { std::printf("unknown case\n"); return 2; }
    std::printf("%d case(s), %d failure(s)\n", ran, failures);
    return failures ? 1 : 0;
}
