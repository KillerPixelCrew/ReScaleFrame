// SPDX-License-Identifier: GPL-3.0-only
// Motion MRT contracts without the game or a GPU: b13 layouts against the shader READMEs and headers, the per-draw
// history policy, the draw-time history, the RTV2 bind-preservation state machine on fake RCache memory, and the
// shader-loader hook contract (caller frame, manifest, original-bytecode proof, reflection rules, disk cache) with
// fake engine frames, a fake compiler and a fake reflector.
#include "motion_constants.h"
#include "motion_foliage.h"
#include "motion_history.h"
#include "motion_policy.h"
#include "motion_profile.h"
#include "motion_target.h"
#include "shader_loader.h"
#include "sr_pass.h"
#include <windows.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <functional>
#include <map>
#include <regex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using namespace rsf::anomaly;
using namespace rsf::anomaly::motion;

namespace {
int checks = 0;
void require(bool condition, const std::string& message)
{
    ++checks;
    if (!condition) throw std::runtime_error(message);
}
std::string read_text(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    require(file.good(), "missing " + path.string());
    return {std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>()};
}
float f32(const std::byte* data, std::size_t offset)
{
    float value;
    std::memcpy(&value, data + offset, sizeof(value));
    return value;
}
temporal::Mat4 sample_matrix(double seed)
{
    temporal::Mat4 m;
    for (std::size_t i = 0; i < 12; ++i) m.values[i] = seed + 0.25 * static_cast<double>(i) - (i % 3 == 0 ? 1.5 : 0.0);
    m.values[12] = m.values[13] = m.values[14] = 0;
    m.values[15] = 1;
    return m;
}

// ---- b13 layouts -----------------------------------------------------------------------------------------------
void layouts_match_readmes()
{
    const std::filesystem::path root = RSF_SOURCE_DIR;
    const auto core = read_text(root / "games/anomaly/shaders/core/README.md");
    const auto foliage = read_text(root / "games/anomaly/shaders/foliage/README.md");
    const auto vertex = read_text(root / "games/anomaly/shaders/core/rsf_core_vertex.h");
    const auto pixel = read_text(root / "games/anomaly/shaders/core/rsf_core_pixel.h");
    const auto row = [&](const std::string& text, const std::string& pattern) {
        std::smatch m;
        require(std::regex_search(text, m, std::regex(pattern)), "README row not found: " + pattern);
        return std::stoul(m[1]);
    };
    require(row(core, R"(\| (\d+) \| 64 \| `rsf_previous_wvp`)") == core_wvp_offset, "core wvp offset");
    require(row(core, R"(\| (\d+) \| 3744 \| `rsf_previous_bones\[234\]`)") == core_bones_offset, "core bones offset");
    require(row(core, R"(\| (\d+) \| 16 \| `rsf_history`)") == core_history_offset, "core history offset");
    require(row(core, R"(dedicated (\d+)-byte constant buffer to \*\*VS b13)") == core_vs_bytes, "core VS size");
    require(row(core, R"(separate (\d+)-byte constant buffer to \*\*PS b13)") == ps_bytes, "PS size");
    require(row(core, R"(\| (\d+) \| 16 \| `rsf_viewport`)") == ps_viewport_offset, "ps viewport");
    require(row(core, R"(\| (\d+) \| 16 \| `rsf_jitter`)") == ps_jitter_offset, "ps jitter");
    require(row(core, R"(\| (\d+) \| 16 \| `rsf_pixel_control`)") == ps_control_offset, "ps control");
    require(row(foliage, R"(bind this (\d+)-byte buffer)") == foliage_vs_bytes, "foliage VS size");
    require(row(foliage, R"(\| (\d+) \| uint4 rsf_foliage_history)") == foliage_history_offset, "foliage history");
    require(row(foliage, R"(\| (\d+) \| float4 rsf_previous_item_state\[61\] \| 976)") + 976 == foliage_vs_bytes, "foliage end");
    // The HLSL packoffsets are the authority FXC reflects.
    require(row(vertex, R"(rsf_previous_wvp : packoffset\(c(\d+)\))") * 16 == core_wvp_offset, "wvp packoffset");
    require(row(vertex, R"(rsf_previous_bones\[234\] : packoffset\(c(\d+)\))") * 16 == core_bones_offset, "bones packoffset");
    require(row(vertex, R"(rsf_history : packoffset\(c(\d+)\))") * 16 == core_history_offset, "history packoffset");
    require(row(pixel, R"(rsf_jitter : packoffset\(c(\d+)\))") * 16 == ps_jitter_offset, "jitter packoffset");
    require(row(pixel, R"(rsf_pixel_control : packoffset\(c(\d+)\))") * 16 == ps_control_offset, "control packoffset");
    require(vertex.find("register(b13)") != std::string::npos && pixel.find("register(b13)") != std::string::npos, "b13 slot");
}

void core_vertex_packing()
{
    CoreVertexBytes bytes;
    const auto wvp = sample_matrix(0.5);
    std::vector<temporal::Mat4> bones{sample_matrix(1), sample_matrix(2), sample_matrix(3)};
    require(pack_core_vertex(wvp, bones, {true, true, false}, bytes), "pack");
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c)
            require(f32(bytes.data(), (r * 4 + c) * 4) == static_cast<float>(wvp.values[r * 4 + c]), "wvp row-major rows");
    for (std::size_t b = 0; b < bones.size(); ++b)
        for (std::size_t r = 0; r < 3; ++r)
            for (std::size_t c = 0; c < 4; ++c)
                require(f32(bytes.data(), core_bones_offset + ((b * 3 + r) * 4 + c) * 4) ==
                    static_cast<float>(bones[b].values[r * 4 + c]), "bone rows 0..2 in native palette order");
    require(f32(bytes.data(), core_bones_offset + 9 * 16) == 0, "unused palette rows zero");
    require(f32(bytes.data(), core_history_offset) == 1 && f32(bytes.data(), core_history_offset + 4) == 1 &&
        f32(bytes.data(), core_history_offset + 8) == 0 && f32(bytes.data(), core_history_offset + 12) == 1, "history lanes");
    std::vector<temporal::Mat4> many(core_max_bones + 1, sample_matrix(1));
    require(!pack_core_vertex(wvp, many, {true, true, false}, bytes), "79 bones refused");
    require(f32(bytes.data(), core_history_offset) == 0 && f32(bytes.data(), core_history_offset + 12) == 1, "refusal is invalid");
    std::vector<temporal::Mat4> exact(core_max_bones, sample_matrix(1));
    require(pack_core_vertex(wvp, exact, {true, true, false}, bytes), "78 bones accepted");
    require(f32(bytes.data(), core_bones_offset + 233 * 16 + 12) == static_cast<float>(exact[77].values[2 * 4 + 3]), "row 233");
    auto bad = wvp;
    bad.values[5] = std::nan("");
    require(!pack_core_vertex(bad, {}, {true, false, true}, bytes), "NaN refused");
    pack_core_invalid(bytes);
    for (std::size_t i = 0; i < core_history_offset + 12; i += 4) require(f32(bytes.data(), i) == 0, "invalid zero");
    FoliageVertexBytes foliage;
    foliage.fill(std::byte{0x7f});
    pack_foliage_invalid(foliage);
    for (const auto b : foliage) require(b == std::byte{}, "foliage zero buffer");
}

void pixel_packing()
{
    PixelBytes bytes;
    require(pack_pixel({2, 3, 800, 400, 0.01f, -0.02f, 0.03f, 0.04f}, bytes), "pixel pack");
    require(f32(bytes.data(), 0) == 2 && f32(bytes.data(), 4) == 3 && f32(bytes.data(), 8) == 1.0f / 800 &&
        f32(bytes.data(), 12) == 1.0f / 400, "viewport origin and reciprocal extent");
    require(f32(bytes.data(), 16) == 0.01f && f32(bytes.data(), 20) == -0.02f && f32(bytes.data(), 24) == 0.03f &&
        f32(bytes.data(), 28) == 0.04f, "current xy, previous zw jitter");
    require(f32(bytes.data(), 32) == 1 && f32(bytes.data(), 36) == 0 && f32(bytes.data(), 44) == 0, "control ABI 1, reserved 0");
    require(!pack_pixel({0, 0, 0, 400}, bytes) && f32(bytes.data(), 32) == 0, "zero extent refused as control 0");
    require(!pack_pixel({0, 0, 1, 1, std::nanf("")}, bytes), "NaN jitter refused");
}

// Engine Fmatrix is row-vector (v * W * V * P); the b13 WVP is the column-vector P * V * W.
void matrix_convention()
{
    const float world[16]{1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 6, 7, 1};   // translation in _41.._43
    const auto w = from_engine(world);
    require(w.values[3] == 5 && w.values[7] == 6 && w.values[11] == 7 && w.values[15] == 1, "translation to column 3");
    const float view[16]{0, 0, 1, 0, 0, 1, 0, 0, -1, 0, 0, 0, 1, 2, 3, 1};
    const float projection[16]{1.5f, 0, 0, 0, 0, 2, 0, 0, 0, 0, 1.01f, 1, 0, 0, -0.2f, 0};
    const auto m = compose(compose(from_engine(projection), from_engine(view)), w);
    // Row-vector reference: clip = (x, y, z, 1) * W * V * P.
    const double p[4]{0.3, -0.7, 2.0, 1.0};
    const float* mats[3]{world, view, projection};
    double v[4]{p[0], p[1], p[2], p[3]};
    for (const auto* mat : mats) {
        double out[4]{};
        for (int c = 0; c < 4; ++c) for (int k = 0; k < 4; ++k) out[c] += v[k] * mat[k * 4 + c];
        std::memcpy(v, out, sizeof(v));
    }
    for (int r = 0; r < 4; ++r) {
        double clip = 0;
        for (int c = 0; c < 4; ++c) clip += m.values[r * 4 + c] * p[c];
        require(std::abs(clip - v[r]) < 1e-5, "column-vector WVP equals the engine's row-vector product");
    }
}

// ---- policy ----------------------------------------------------------------------------------------------------
void core_policy()
{
    using geometry::DrawKind;
    using geometry::DrawVerdict;
    CameraSample camera{sample_matrix(0.1), sample_matrix(0.2)};
    temporal::InstanceSnapshot rigid;
    rigid.world = sample_matrix(3);
    temporal::InstanceSnapshot skinned = rigid;
    skinned.kind = temporal::InstanceKind::Skinned;
    skinned.palette_layout = 7;
    skinned.bones = {sample_matrix(4), sample_matrix(5)};
    CoreDrawInput base{Family::CoreStatic, -2, true, DrawVerdict::IgnoredStaticWorld, DrawKind::Rigid, true, false, &camera, nullptr};
    auto d = decide_core(base);
    require(d.reason == HistoryReason::Valid && d.flags.draw && !d.flags.palette, "static world valid");
    const auto vp = compose(camera.projection, camera.view);
    require(d.previous_wvp == vp, "static world previous WVP = P*V (world identity)");
    auto in = base; in.in_visual = false;
    require(decide_core(in).reason == HistoryReason::NoVisual, "outside a visual");
    in = base; in.verdict = DrawVerdict::UnidentifiedDynamic;
    require(decide_core(in).reason == HistoryReason::Unidentified, "unidentified dynamic invalid");
    in = base; in.previous_camera = nullptr;
    require(decide_core(in).reason == HistoryReason::NoPreviousCamera, "first frame invalid");
    in = base; in.camera_conflict = true;
    require(decide_core(in).reason == HistoryReason::CameraConflict, "camera conflict invalid");
    in = base; in.context = false;
    require(decide_core(in).reason == HistoryReason::NoContext, "no context invalid");
    in = base; in.verdict = DrawVerdict::Identified;
    require(decide_core(in).reason == HistoryReason::NoPreviousInstance, "identified without previous");
    in.previous = &rigid;
    d = decide_core(in);
    require(d.reason == HistoryReason::Valid && d.previous_wvp == compose(vp, rigid.world), "rigid previous world");
    in.family = Family::CoreModel; in.skin = -1;
    d = decide_core(in);
    require(d.reason == HistoryReason::Valid && d.flags.rigid, "SKIN_NONE rigid flag");
    in.skin = 2;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "rigid draw on skinned permutation");
    in.kind = DrawKind::HardwareSkinned; in.previous = &skinned;
    d = decide_core(in);
    require(d.reason == HistoryReason::Valid && d.flags.palette && d.bones.size() == 2 && !d.flags.rigid, "skinned palette");
    in.skin = 0;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "a palette draw on the SKIN_0 permutation");
    // CSkeletonX RM_SINGLE: the engine folds the bone into the draw's world before the draw, the snapshot holds that
    // bone-composed world, and SKIN_0 has no palette. Valid with the previous composed world and no palette flag.
    in.kind = DrawKind::SingleBone;
    auto single = rigid;
    single.world = sample_matrix(6);
    in.previous = &single;
    d = decide_core(in);
    require(d.reason == HistoryReason::Valid && d.flags.draw && !d.flags.palette && !d.flags.rigid && d.bones.empty() &&
        d.previous_wvp == compose(vp, single.world), "single bone: previous bone-composed world, no palette");
    in.skin = 2;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "single bone on a palette permutation");
    in.skin = -1;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "single bone on SKIN_NONE");
    in.family = Family::CoreStatic; in.skin = -2;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "single bone on a static root");
    in.family = Family::CoreModel; in.skin = 0; in.previous = nullptr;
    require(decide_core(in).reason == HistoryReason::NoPreviousInstance, "single bone without previous");
    in.verdict = DrawVerdict::IgnoredStaticWorld;
    require(decide_core(in).reason == HistoryReason::KindMismatch, "single bone never takes the static-world shortcut");
    in.verdict = DrawVerdict::Identified;
    in.kind = DrawKind::SoftwareSkinned; in.skin = -1;
    require(decide_core(in).reason == HistoryReason::UnsupportedKind, "software skin unsupported");
    in.kind = DrawKind::HardwareSkinned; in.skin = 1;
    auto large = skinned;
    large.bones.assign(core_max_bones + 1, sample_matrix(1));
    in.previous = &large;
    require(decide_core(in).reason == HistoryReason::TooManyBones, "palette overflow");
}

// ---- a moving skinned character, end to end ----------------------------------------------------------------------
// The adapter chain for one vertex of an NPC that walks while the camera pans: MotionHistory (two frames), decide_core,
// pack_core_vertex and pack_pixel bytes, the core shader formulas (skinned previous position from the packed palette,
// rsf_encode_motion from core/README.md), the pipeline's decode (sr_pass.h decode_reference) and the dense resolve's
// written-vector mapping (decoded UV * -render size). The result must be the true previous-minus-current unjittered
// pixel displacement of that material point. The same pixel without a valid vector is reconstructed from depth with the
// camera alone, and for a mover that is wrong by the object's own motion: that is the ghost.
namespace mover {
using temporal::Mat4;
constexpr double W = 960, H = 540;
Mat4 translation(double x, double y, double z)
{
    Mat4 m = Mat4::identity();
    m.values[3] = x; m.values[7] = y; m.values[11] = z;
    return m;
}
Mat4 yaw(double radians)
{
    Mat4 m = Mat4::identity();
    m.values[0] = std::cos(radians); m.values[2] = std::sin(radians);
    m.values[8] = -std::sin(radians); m.values[10] = std::cos(radians);
    return m;
}
// X-Ray build_projection_HAT in the column-vector convention (clip.w = view z, standard depth).
Mat4 projection(double fov = 1.2, double n = 0.2, double f = 900)
{
    const double h = 1 / std::tan(fov / 2), w = h / (W / H), q = f / (f - n);
    Mat4 m{};
    m.values[0] = w; m.values[5] = h; m.values[10] = q; m.values[11] = -q * n; m.values[14] = 1;
    return m;
}
// Stage 3: the pixel jitter (render pixels, +X right, +Y down) added to clip.xy / w as (2u, -2v), u = jx / W.
Mat4 jittered(const Mat4& p, double jx, double jy)
{
    Mat4 m = p;
    for (std::size_t c = 0; c < 4; ++c) {
        m.values[c] += 2 * jx / W * p.values[12 + c];
        m.values[4 + c] -= 2 * jy / H * p.values[12 + c];
    }
    return m;
}
std::array<double, 4> transform_point(const Mat4& m, const std::array<double, 4>& v)
{
    std::array<double, 4> out{};
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) out[r] += m.values[r * 4 + c] * v[c];
    return out;
}
// Pixel (x right, y down) of a clip position, viewport origin zero.
std::array<double, 2> pixel(const std::array<double, 4>& clip)
{
    return {(clip[0] / clip[3] * 0.5 + 0.5) * W, (0.5 - clip[1] / clip[3] * 0.5) * H};
}
// Row r of a packed row_major float4x4 or float4 array, as the shader reads it.
std::array<double, 4> row(const std::byte* bytes, std::size_t offset, std::size_t r)
{
    std::array<double, 4> out{};
    for (std::size_t c = 0; c < 4; ++c) out[c] = f32(bytes, offset + (r * 4 + c) * 4);
    return out;
}
double dot(const std::array<double, 4>& a, const std::array<double, 4>& b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]; }

struct Frame {
    Mat4 view, projection, world;
    std::vector<Mat4> bones;
    double jx{}, jy{};
};
struct Result {
    double written[2]{}, truth[2]{}, camera_only[2]{};
};
// skin 2: two-bone palette blend (SKIN_2). skin 0: CSkeletonX RM_SINGLE, the bone folded into the draw's world.
Result run(int skin)
{
    // Frame a, then b. The NPC walks 0.4 m to the right and turns, its bones swing, the camera pans and moves.
    const Mat4 p = projection();
    const Frame a{compose(yaw(0.00), translation(-0.1, -1.7, 0)), p, compose(translation(1.0, 0, 8), yaw(0.10)),
        {translation(0, 0.02, 0), yaw(0.20)}, 0.25, -0.375};
    const Frame b{compose(yaw(0.02), translation(-0.15, -1.7, 0)), p, compose(translation(1.4, 0, 8), yaw(0.18)),
        {translation(0, 0.05, 0), yaw(0.35)}, -0.4375, 0.125};
    const std::array<double, 4> vertex{0.3, 1.2, 0.1, 1};
    const double weight = 0.35;   // weight of bone 1
    const auto skinned = [&](const Frame& f) {
        std::array<double, 4> out{0, 0, 0, 1};
        const auto p0 = transform_point(f.bones[0], vertex), p1 = transform_point(f.bones[1], vertex);
        for (int i = 0; i < 3; ++i) out[i] = (1 - weight) * p0[i] + weight * p1[i];
        return out;
    };
    // The draw's world at the flush: the actor world, or for RM_SINGLE the bone-composed world.
    const auto draw_world = [&](const Frame& f) { return skin == 0 ? compose(f.world, f.bones[1]) : f.world; };
    const auto object_point = [&](const Frame& f) { return skin == 0 ? vertex : skinned(f); };

    MotionHistory history;
    const temporal::InstanceKey key{0x10, 0x20, 1};
    const auto snapshot = [&](const Frame& f) {
        temporal::InstanceSnapshot s;
        s.world = draw_world(f);
        if (skin != 0) { s.kind = temporal::InstanceKind::Skinned; s.palette_layout = 1; s.bones = f.bones; }
        return s;
    };
    const auto camera = [&](const Frame& f) { return CameraSample{f.view, jittered(f.projection, f.jx, f.jy)}; };
    history.begin_frame(temporal::ViewId::Main, 100, 0, unsigned(W), unsigned(H));
    history.observe_camera(temporal::ViewId::Main, temporal::ProjectionDomain::World, camera(a));
    history.record(temporal::ViewId::Main, key, snapshot(a));
    require(history.begin_frame(temporal::ViewId::Main, 101, 0, unsigned(W), unsigned(H)), "mover: continuous frame");
    history.observe_camera(temporal::ViewId::Main, temporal::ProjectionDomain::World, camera(b));
    const auto* previous = history.record(temporal::ViewId::Main, key, snapshot(b));
    require(previous != nullptr, "mover: previous snapshot");

    CoreDrawInput in{Family::CoreModel, skin, true, geometry::DrawVerdict::Identified,
        skin == 0 ? geometry::DrawKind::SingleBone : geometry::DrawKind::HardwareSkinned, true, false,
        history.previous_camera(temporal::ViewId::Main, temporal::ProjectionDomain::World), previous};
    const auto decision = decide_core(in);
    require(decision.reason == HistoryReason::Valid, std::string("mover: valid history, got ") + history_reason_name(decision.reason));
    CoreVertexBytes vbytes;
    require(pack_core_vertex(decision.previous_wvp, decision.bones, decision.flags, vbytes), "mover: pack VS b13");
    require(f32(vbytes.data(), core_history_offset) == 1 && f32(vbytes.data(), core_history_offset + 4) == (skin == 0 ? 0 : 1) &&
        f32(vbytes.data(), core_history_offset + 12) == 1, "mover: history flags");

    // VS: previous clip = rsf_previous_wvp * (previous-palette skinning of the vertex), read back from the packed bytes.
    std::array<double, 4> previous_object = vertex;
    if (skin != 0)
        for (int r = 0; r < 3; ++r)
            previous_object[r] = (1 - weight) * dot(row(vbytes.data(), core_bones_offset, r), vertex) +
                weight * dot(row(vbytes.data(), core_bones_offset, 3 + r), vertex);
    std::array<double, 4> previous_clip{};
    for (int r = 0; r < 4; ++r) previous_clip[r] = dot(row(vbytes.data(), core_wvp_offset, r), previous_object);
    // Rasterized position of the current, jittered draw.
    const auto current_clip = transform_point(compose(compose(jittered(b.projection, b.jx, b.jy), b.view), draw_world(b)), object_point(b));
    const auto raster = pixel(current_clip);
    // PS b13 and rsf_encode_motion.
    PixelBytes pbytes;
    require(pack_pixel({0, 0, float(W), float(H), float(b.jx / W), float(b.jy / H), float(a.jx / W), float(a.jy / H)}, pbytes),
        "mover: pack PS b13");
    const double rcp[2]{f32(pbytes.data(), ps_viewport_offset + 8), f32(pbytes.data(), ps_viewport_offset + 12)};
    const double cj[2]{f32(pbytes.data(), ps_jitter_offset), f32(pbytes.data(), ps_jitter_offset + 4)};
    const double pj[2]{f32(pbytes.data(), ps_jitter_offset + 8), f32(pbytes.data(), ps_jitter_offset + 12)};
    const double current_uv[2]{raster[0] * rcp[0] - cj[0], raster[1] * rcp[1] - cj[1]};
    const double previous_uv[2]{previous_clip[0] / previous_clip[3] * 0.5 + 0.5 - pj[0],
        previous_clip[1] / previous_clip[3] * -0.5 + 0.5 - pj[1]};
    const float stored[4]{float(current_uv[0] - previous_uv[0]), float(current_uv[1] - previous_uv[1]), 1, 0};
    // Pipeline: decode, then the dense resolve's written-vector mapping (previous-minus-current render pixels).
    const auto decoded = sr::decode_reference(stored);
    require(!decoded.unwritten, "mover: a valid vector is not the sentinel");
    Result result;
    result.written[0] = -double(decoded.x) * W * sr::motion_to_uv[0];
    result.written[1] = -double(decoded.y) * H * sr::motion_to_uv[1];
    // Truth: the same material point through both unjittered cameras.
    const auto truth_previous = pixel(transform_point(compose(compose(a.projection, a.view), draw_world(a)), object_point(a)));
    const auto truth_current = pixel(transform_point(compose(compose(b.projection, b.view), draw_world(b)), object_point(b)));
    result.truth[0] = truth_previous[0] - truth_current[0];
    result.truth[1] = truth_previous[1] - truth_current[1];
    // Camera-only reconstruction of the same pixel (what an invalid pixel becomes): the current world point, assumed
    // static, through the previous camera.
    const auto world_point = transform_point(draw_world(b), object_point(b));
    const auto camera_previous = pixel(transform_point(compose(a.projection, a.view), world_point));
    result.camera_only[0] = camera_previous[0] - truth_current[0];
    result.camera_only[1] = camera_previous[1] - truth_current[1];
    return result;
}
}
void moving_character()
{
    for (const int skin : {2, 0}) {
        const auto r = mover::run(skin);
        const double error = std::hypot(r.written[0] - r.truth[0], r.written[1] - r.truth[1]);
        const double ghost = std::hypot(r.camera_only[0] - r.truth[0], r.camera_only[1] - r.truth[1]);
        std::printf("mover SKIN_%d: written (%.4f, %.4f) px, truth (%.4f, %.4f) px, error %.5f px; camera-only misses by %.2f px\n",
            skin, r.written[0], r.written[1], r.truth[0], r.truth[1], error, ghost);
        require(error < 2e-3, "mover: written vector equals the true unjittered displacement, SKIN_" + std::to_string(skin));
        require(std::hypot(r.truth[0], r.truth[1]) > 5, "mover: the fixture actually moves");
        require(ghost > 5, "mover: camera-only reconstruction is wrong for a mover (the ghost a valid vector prevents)");
    }
}

// ---- history ---------------------------------------------------------------------------------------------------
void history_rules()
{
    using temporal::ProjectionDomain;
    using temporal::ViewId;
    MotionHistory h;
    const temporal::InstanceKey key{1, 2, 3};
    temporal::InstanceSnapshot snap;
    snap.world = sample_matrix(1);
    const CameraSample a{sample_matrix(0.1), sample_matrix(0.2)}, b{sample_matrix(0.3), sample_matrix(0.4)};
    require(!h.begin_frame(ViewId::Main, 10, 0, 640, 360), "no previous on first frame");
    require(h.observe_camera(ViewId::Main, ProjectionDomain::World, a) == CameraStatus::Recorded, "record camera");
    require(h.observe_camera(ViewId::Main, ProjectionDomain::World, a) == CameraStatus::Same, "same camera");
    require(h.record(ViewId::Main, key, snap) == nullptr, "nothing previous yet");
    require(h.begin_frame(ViewId::Main, 10, 0, 640, 360) == false, "split scene re-open is the same frame");
    require(h.begin_frame(ViewId::Main, 11, 0, 640, 360), "continuous frame");
    require(h.previous_camera(ViewId::Main, ProjectionDomain::World) && *h.previous_camera(ViewId::Main, ProjectionDomain::World) == a, "previous camera");
    require(!h.previous_camera(ViewId::Main, ProjectionDomain::FirstPerson), "no first-person camera last frame");
    require(!h.previous_camera(ViewId::Scope, ProjectionDomain::World), "scope view independent");
    auto moved = snap;
    moved.world.values[3] = 9;
    const auto* previous = h.record(ViewId::Main, key, moved);
    require(previous && previous->world == snap.world, "previous snapshot of the same key");
    require(h.record(ViewId::Main, key, moved) == previous, "identical repeat");
    auto conflicting = moved;
    conflicting.world.values[7] = 4;
    require(h.record(ViewId::Main, key, conflicting) == nullptr && h.conflicts() == 1, "same-frame conflict");
    require(h.observe_camera(ViewId::Main, ProjectionDomain::World, a) == CameraStatus::Recorded, "camera frame 11");
    require(h.observe_camera(ViewId::Main, ProjectionDomain::World, b) == CameraStatus::Conflict, "camera conflict");
    h.begin_frame(ViewId::Main, 12, 0, 640, 360);
    require(h.record(ViewId::Main, key, moved) == nullptr, "conflicted slot gives no history");
    require(!h.previous_camera(ViewId::Main, ProjectionDomain::World), "conflicted camera gives no history");
    h.begin_frame(ViewId::Main, 13, 0, 640, 360);
    require(h.record(ViewId::Main, key, moved) != nullptr, "history resumes");
    auto skinned = moved;
    skinned.kind = temporal::InstanceKind::Skinned; skinned.palette_layout = 1; skinned.bones = {sample_matrix(2)};
    h.begin_frame(ViewId::Main, 14, 0, 640, 360);
    require(h.record(ViewId::Main, key, skinned) == nullptr, "kind change gives no history");
    h.begin_frame(ViewId::Main, 15, 0, 1280, 720);
    require(h.record(ViewId::Main, key, skinned) == nullptr, "resize drops history");
    h.begin_frame(ViewId::Main, 16, 1, 1280, 720);
    require(h.record(ViewId::Main, key, skinned) == nullptr, "reset generation drops history");
    h.begin_frame(ViewId::Main, 17, 1, 1280, 720);
    require(h.record(ViewId::Main, key, skinned) != nullptr, "steady again");
    h.retire(key);
    h.begin_frame(ViewId::Main, 18, 1, 1280, 720);
    require(h.record(ViewId::Main, key, skinned) == nullptr, "retired generation refused");
    require(h.record(ViewId::Main, {1, 2, 4}, skinned) == nullptr, "new generation has no inherited history");
    h.begin_frame(ViewId::Main, 19, 1, 1280, 720);
    require(h.record(ViewId::Main, {1, 2, 4}, skinned) != nullptr, "new generation builds history");
    require(!h.begin_frame(ViewId::Main, 5, 1, 1280, 720), "older frame number restarts history");
}

// ---- RTV2 binder -----------------------------------------------------------------------------------------------
struct FakeBackend {
    std::array<std::byte, 0x1000> memory{};
    static constexpr BackendLayout layout{0x578, 0x598, 0x980};
    void* rt(int i) { void* v; std::memcpy(&v, memory.data() + 0x578 + i * 8, 8); return v; }
    void set_rt(int i, void* v) { std::memcpy(memory.data() + 0x578 + i * 8, &v, 8); }
    bool changed() { return memory[0x980] != std::byte{}; }
    void set_changed(bool v) { memory[0x980] = std::byte{v}; }
    void set_zb(void* v) { std::memcpy(memory.data() + 0x598, &v, 8); }
    // CBackend::set_RT + ApplyRTandZB, as the engine does at a u_setrt.
    void engine_bind(void* rt0, void* rt1, void* rt2) {
        set_rt(0, rt0); set_rt(1, rt1); set_rt(2, rt2); set_changed(false);
    }
};
struct FakeHost final : BinderHost {
    FakeBackend* backend{};
    int binds{}, clears{};
    void* last[4]{};
    void* last_depth{};
    bool fail_write{};
    bool read(const void* a, void* o, std::size_t s) noexcept override { std::memcpy(o, a, s); return true; }
    bool write(void* a, const void* i, std::size_t s) noexcept override { if (fail_write) return false; std::memcpy(a, i, s); return true; }
    void set_targets(void* const (&t)[4], void* d) noexcept override { ++binds; std::memcpy(last, t, sizeof(last)); last_depth = d; }
    void clear_targets() noexcept override { ++clears; }
};
void binder_state_machine()
{
    FakeBackend engine;
    FakeHost host;
    MotionBinder binder(host, FakeBackend::layout);
    auto* base = engine.memory.data();
    void* const position = reinterpret_cast<void*>(0x1000), *color = reinterpret_cast<void*>(0x2000);
    void* const motion = reinterpret_cast<void*>(0x3000), *zb = reinterpret_cast<void*>(0x4000), *other = reinterpret_cast<void*>(0x5000);
    engine.set_zb(zb);
    engine.engine_bind(position, color, nullptr);
    require(binder.on_draw(base, true, position, motion) == BindAction::Bound, "first G-buffer draw binds");
    require(engine.rt(2) == motion && !engine.changed(), "cache holds motion, clean");
    require(host.binds == 1 && host.last[0] == position && host.last[1] == color && host.last[2] == motion && host.last_depth == zb,
        "the same targets plus slot 2 are bound");
    require(binder.on_draw(base, true, position, motion) == BindAction::AlreadyBound && host.binds == 1, "kept without a call");
    // An engine bind inside the pass (u_setrt to the same G-buffer) drops slot 2; the next draw restores it.
    engine.engine_bind(position, color, nullptr);
    require(binder.on_draw(base, true, position, motion) == BindAction::Bound && host.binds == 2, "rebind after engine bind");
    // A pass of another RT0 inside the window (occlusion queries) must not inherit the motion target.
    engine.set_rt(0, other);
    require(binder.on_draw(base, true, position, motion) == BindAction::Unbound && engine.rt(2) == nullptr &&
        host.last[2] == nullptr && host.last[0] == other, "foreign RT0 unbinds");
    require(binder.on_draw(base, true, position, motion) == BindAction::None, "nothing to do");
    // A foreign slot-2 target is never replaced.
    engine.engine_bind(position, color, other);
    require(binder.on_draw(base, true, position, motion) == BindAction::Conflict && engine.rt(2) == other && !binder.motion_bound(),
        "conflict");
    // Not wanted (shadow pass, unusable target) while bound: removed.
    engine.engine_bind(position, color, nullptr);
    binder.on_draw(base, true, position, motion);
    require(binder.on_draw(base, false, position, motion) == BindAction::Unbound && engine.rt(2) == nullptr, "unwanted unbinds");
    // Window close outside a draw: drop from the cache like set_RT does.
    binder.on_draw(base, true, position, motion);
    const int clears = host.clears;
    require(binder.close(base, motion) == BindAction::Unbound && engine.rt(2) == nullptr && engine.changed() &&
        host.clears == clears + 1, "close dirties a clean cache and unbinds");
    engine.engine_bind(position, color, nullptr);
    binder.on_draw(base, true, position, motion);
    engine.set_changed(true);
    require(binder.close(base, motion) == BindAction::Unbound && host.clears == clears + 1, "already dirty: no extra unbind");
    require(binder.close(base, motion) == BindAction::None, "close with nothing bound");
    engine.engine_bind(position, color, nullptr);
    host.fail_write = true;
    require(binder.on_draw(base, true, position, motion) == BindAction::Fault && engine.rt(2) == nullptr, "write fault changes nothing");
}

// ---- shader loader ---------------------------------------------------------------------------------------------
std::vector<std::byte> bytes_of(const std::string& text)
{
    std::vector<std::byte> v(text.size());
    std::memcpy(v.data(), text.data(), text.size());
    return v;
}
std::string sha(const std::string& text) { return sha256_hex(std::as_bytes(std::span(text))); }

// Fake compile: "DXBC|<tree leaf>|<root>|<entry>|<target>|<flags>|<defines>"; the include tree decides the result.
struct FakeCompiler final : Compiler {
    int calls{};
    bool fail_motion{};
    std::string drift;   // appended to "original" output to model a different compiler/includes
    bool compile(const std::filesystem::path& tree, std::span<const std::byte> root, const ShaderRequest& r,
        std::vector<std::byte>& out, std::string& error) override
    {
        ++calls;
        const auto leaf = tree.filename().string();
        if (leaf == "motion" && fail_motion) { error = "fake failure"; return false; }
        std::string text = "DXBC|" + leaf + "|" + std::string(reinterpret_cast<const char*>(root.data()), root.size()) + "|" +
            r.entry + "|" + r.target + "|" + std::to_string(r.flags) + "|";
        for (const auto& d : r.defines) text += d.name + "=" + d.value + ";";
        if (leaf == "original") text += drift;
        out = bytes_of(text);
        return true;
    }
};
Reflection stock_reflection(char stage)
{
    Reflection r;
    r.version = stage == 'v' ? 0x10050 : 0x50;
    r.bindings = {{"s_base", 3, 0, 1, 0, 0}, {"$Globals", 0, 0, 1, 0, 0}, {"dynamic", 0, 1, 1, 0, 0}};
    r.buffers = {{"$Globals", 64, {{"m_WVP", 0, 64}}}, {"dynamic", 32, {{"L_hemi", 0, 16}}}};
    if (stage == 'v') {
        r.inputs = {{"POSITION", 0, 0, 0, 0, 15}, {"NORMAL", 0, 1, 0, 3, 15}};
        r.outputs = {{"TEXCOORD", 0, 0, 0, 3, 3}, {"SV_Position", 0, 1, 1, 3, 15}};
    } else {
        r.inputs = {{"TEXCOORD", 0, 0, 0, 3, 3}, {"SV_Position", 0, 1, 1, 3, 15}};
        r.outputs = {{"SV_Target", 0, 0, 64, 3, 15}, {"SV_Target", 1, 1, 64, 3, 15}};
    }
    return r;
}
Reflection variant_reflection(char stage)
{
    auto r = stock_reflection(stage);
    r.bindings.push_back({stage == 'v' ? "RSFCorePrevious" : "RSFCorePixel", 0, 13, 1, 0, 0});
    if (stage == 'v') {
        r.outputs = {{"TEXCOORD", 12, 0, 0, 3, 15}, {"TEXCOORD", 13, 1, 0, 3, 1}, {"TEXCOORD", 0, 2, 0, 3, 3}, {"SV_Position", 0, 3, 1, 3, 15}};
    } else {
        r.inputs = {{"TEXCOORD", 12, 0, 0, 3, 15}, {"TEXCOORD", 13, 1, 0, 3, 1}, {"TEXCOORD", 0, 2, 0, 3, 3}, {"SV_Position", 0, 3, 1, 3, 15}};
        r.outputs.push_back({"SV_Target", 2, 2, 64, 3, 15});
    }
    return r;
}
struct FakeReflector final : Reflector {
    std::function<void(char, bool, Reflection&)> edit;
    bool reflect(std::span<const std::byte> code, Reflection& out) override
    {
        const std::string text(reinterpret_cast<const char*>(code.data()), code.size());
        if (text.rfind("DXBC|", 0) && text.rfind("STOCK|", 0)) return false;
        const bool variant = text.find("|motion|") != std::string::npos;
        const char stage = text.find("|vs_5_0|") != std::string::npos ? 'v' : 'p';
        out = variant ? variant_reflection(stage) : stock_reflection(stage);
        if (edit) edit(stage, variant, out);
        return true;
    }
};

void compatibility_rules()
{
    for (const char stage : {'v', 'p'}) {
        const auto stock = stock_reflection(stage);
        auto variant = variant_reflection(stage);
        require(compatible(stage, stock, variant) == Compatibility::Ok, "reference variant compatible");
        auto v = variant; v.bindings[0].point = 1;
        require(compatible(stage, stock, v) == Compatibility::MissingBinding, "moved stock binding");
        v = variant; v.bindings.push_back({"RSFExtra", 0, 12, 1, 0, 0});
        require(compatible(stage, stock, v) == Compatibility::ExtraBinding, "unknown extra buffer");
        v = variant; v.bindings.back().point = 12;
        require(compatible(stage, stock, v) == Compatibility::ExtraBinding, "RSF buffer not at b13");
        v = variant; v.bindings.back().name = stage == 'v' ? "RSFCorePixel" : "RSFCorePrevious";
        require(compatible(stage, stock, v) == Compatibility::ExtraBinding, "wrong-stage RSF buffer");
        v = variant; v.buffers[0].variables[0].offset = 16;
        require(compatible(stage, stock, v) == Compatibility::Buffer, "moved engine constant");
        v = variant; v.buffers[1].size = 48;
        require(compatible(stage, stock, v) == Compatibility::Buffer, "resized engine buffer");
        v = variant; v.version = 0;
        require(compatible(stage, stock, v) == Compatibility::Version, "shader model");
    }
    auto vs = variant_reflection('v');
    vs.inputs[1].component = 1;
    require(compatible('v', stock_reflection('v'), vs) == Compatibility::Inputs, "VS input signature must be identical");
    vs = variant_reflection('v');
    vs.outputs.erase(vs.outputs.begin() + 2);
    require(compatible('v', stock_reflection('v'), vs) == Compatibility::Outputs, "VS lost a native output");
    auto ps = variant_reflection('p');
    ps.outputs.back().index = 3; ps.outputs.back().reg = 3;
    require(compatible('p', stock_reflection('p'), ps) == Compatibility::Outputs, "motion must be SV_Target2");
    ps = variant_reflection('p');
    ps.outputs.push_back({"SV_Depth", 0, ~0u, 0, 3, 1});
    require(compatible('p', stock_reflection('p'), ps) == Compatibility::Outputs, "only SV_Target2 may be added");
    require(linkable(variant_reflection('v'), variant_reflection('p')), "variant pair links");
    require(!linkable(stock_reflection('v'), variant_reflection('p')), "stock VS cannot feed a motion PS");
    require(!linkable(variant_reflection('v'), stock_reflection('p')), "motion VS registers shift under a stock PS");
}

struct FakeFrame {
    const MotionProfile* profile = &motion_standard;
    std::vector<std::byte> module = std::vector<std::byte>(0x900000);
    std::vector<std::byte> stack = std::vector<std::byte>(0x2000);
    std::string source, entry = "main", flags_text;
    std::vector<std::pair<std::string, std::string>> defines;
    std::vector<std::string> storage;
    std::vector<const char*> pointers;
    void build(bool terminate = true)
    {
        auto put = [&](std::size_t offset, const void* value, std::size_t size) { std::memcpy(stack.data() + offset, value, size); };
        const char* src = source.c_str();
        const std::uint32_t length = static_cast<std::uint32_t>(source.size());
        const char* e = entry.c_str();
        const std::uint32_t flags = 8;   // D3D10_SHADER_PACK_MATRIX_ROW_MAJOR
        put(profile->frame.source_offset, &src, 8);
        put(profile->frame.length_offset, &length, 4);
        put(profile->frame.entry_offset, &e, 8);
        put(profile->frame.flags_offset, &flags, 4);
        storage.clear();
        storage.reserve(defines.size() * 2);
        std::size_t i = 0;
        for (const auto& [name, value] : defines) {
            storage.push_back(name); storage.push_back(value);
            const char* pair[2]{storage[storage.size() - 2].c_str(), storage.back().c_str()};
            put(profile->frame.defines_offset + i++ * 16, pair, 16);
        }
        const char* none[2]{terminate ? nullptr : "X", "1"};
        for (std::size_t j = i; j < frame_defines_capacity; ++j) {
            put(profile->frame.defines_offset + j * 16, none, 16);
            if (terminate) break;
        }
    }
    const void* return_address(std::size_t call) const { return module.data() + profile->create_shader_calls[call].rva + call_length; }
};
FrameReader direct = [](const void* a, void* o, std::size_t s) { if (!a) return false; std::memcpy(o, a, s); return true; };

// Both exact builds (build_profile.h): each accepts only its own two call sites and reads its own frame offsets.
void caller_frame(const MotionProfile& profile, const MotionProfile& other)
{
    FakeFrame frame;
    frame.profile = &profile;
    frame.source = "#include \"common.h\"\nmain body";
    frame.defines = {{"SMAP_size", "2048"}, {"SKIN_2", "1"}, {"USE_SUNFILTER", "1"}};
    frame.build();
    for (std::size_t call = 0; call < profile.create_shader_calls.size(); ++call) {
        ShaderRequest request;
        require(read_request(profile, frame.module.data(), frame.return_address(call), frame.stack.data(), direct, request) == FrameStatus::Ok,
            "both create_shader call sites are accepted");
        require(std::string(reinterpret_cast<const char*>(request.source.data()), request.source.size()) == frame.source, "source");
        require(request.entry == "main" && request.flags == 8, "entry and flags");
        require(request.defines.size() == 3 && request.defines[1].name == "SKIN_2" && request.defines[0].value == "2048",
            "defines in engine order");
        require(skin_mode(request.defines) == 2, "skin mode from defines");
    }
    ShaderRequest request;
    require(read_request(profile, frame.module.data(), frame.module.data() + 0x1234, frame.stack.data(), direct, request) ==
        FrameStatus::UnknownCaller, "any other return address is refused");
    require(read_request(profile, frame.module.data(), static_cast<const std::byte*>(frame.return_address(0)) - 5, frame.stack.data(), direct,
        request) == FrameStatus::UnknownCaller, "the call instruction itself is not a return address");
    FakeFrame unterminated = frame;
    unterminated.build(false);
    require(read_request(profile, unterminated.module.data(), unterminated.return_address(0), unterminated.stack.data(), direct, request) ==
        FrameStatus::BadDefines, "a define list without terminator is refused");
    const FrameReader faulting = [&](const void* a, void* o, std::size_t s) {
        if (a == frame.stack.data() + profile.frame.entry_offset) return false;
        std::memcpy(o, a, s); return true;
    };
    require(read_request(profile, frame.module.data(), frame.return_address(1), frame.stack.data(), faulting, request) == FrameStatus::Unreadable,
        "unreadable frame");
    std::uint32_t zero = 0;
    std::memcpy(frame.stack.data() + profile.frame.length_offset, &zero, 4);
    require(read_request(profile, frame.module.data(), frame.return_address(1), frame.stack.data(), direct, request) == FrameStatus::BadSource,
        "empty source");
    // The other build's profile names other call sites: a return address of this build is an unknown caller there.
    require(read_request(other, frame.module.data(), frame.return_address(0), frame.stack.data(), direct, request) ==
        FrameStatus::UnknownCaller, "a call site of one build is refused under the other build's profile");
}

void caller_frames()
{
    caller_frame(motion_standard, motion_avx);
    caller_frame(motion_avx, motion_standard);
}

void manifest_rules()
{
    const std::string ns(64, 'a'), h1(64, '1'), h2(64, '2');
    std::string error;
    auto ok = Manifest::parse("rsf-anomaly-motion-manifest 1\r\nnamespace " + ns + "\nroot vs core_static deffer_base_flat.vs " + h1 +
        " " + h2 + "\n", error);
    require(ok && ok->by_original.size() == 1 && ok->by_original.at(h1).family == Family::CoreStatic && ok->name_space == ns, "parse");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 2\n", error), "version");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot ps core_static x.ps " + h1 + " " + h2 + "\n", error),
        "family/stage mismatch");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot vs core_static ../x.vs " + h1 + " " + h2 + "\n", error),
        "path escape");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot vs core_static a.vs " + h1 + " " + h2 +
        "\nroot vs core_model b.vs " + h1 + " " + h2 + "\n", error), "duplicate original");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\n", error), "empty");
    require(!Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot vs core_static a.vs " + h1 + " XYZ\n", error),
        "bad hash");
}

void loader_contract()
{
    wchar_t temp[MAX_PATH]{};
    GetTempPathW(MAX_PATH, temp);
    const auto root = std::filesystem::path(temp) / ("rsf-motion-contract-" + std::to_string(GetCurrentProcessId()));
    std::filesystem::remove_all(root);
    const std::string vs_source = "VS root source", ps_source = "PS root source", vs_motion = "VS motion root";
    std::filesystem::create_directories(root / "motion" / "r3");
    std::ofstream(root / "motion" / "r3" / "deffer_base_flat.vs", std::ios::binary) << vs_motion;
    const std::string ns(64, 'c');
    std::string error;
    auto manifest = Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot vs core_static deffer_base_flat.vs " +
        sha(vs_source) + " " + sha(vs_motion) + "\nroot ps core_pixel deffer_base_flat.ps " + sha(ps_source) + " " + std::string(64, 'e') + "\n",
        error);
    require(manifest.has_value(), "fixture manifest: " + error);
    FakeCompiler compiler;
    FakeReflector reflector;
    ShaderLoader loader(*manifest, root, root / "cache", compiler, reflector);
    ShaderRequest request;
    request.target = "vs_5_0";
    request.entry = "main";
    request.flags = 8;
    request.defines = {{"SMAP_size", "2048"}};
    request.source = bytes_of("unrelated root");
    require(loader.prepare(request).outcome == Outcome::NotAllowlisted && compiler.calls == 0, "unlisted root untouched");
    request.source = bytes_of(vs_source);
    std::vector<std::byte> engine_code;
    compiler.compile(root / "original", request.source, request, engine_code, error);
    compiler.calls = 0;
    request.bytecode = engine_code;
    {
        unsigned delegated = 0;
        bool refuse = false;
        ShaderLoader runtime_loader(*manifest, compiler, reflector,
            [&](const ShaderRequest& input, const ManifestEntry& entry) {
                ++delegated;
                require(&input == &request && input.flags == 8 && input.entry == "main" &&
                    input.defines.size() == 1 && input.defines[0].value == "2048" &&
                    entry.family == Family::CoreStatic, "Runtime delegate receives exact engine inputs and eligibility");
                Prepared result;
                result.entry = &entry;
                result.outcome = refuse ? Outcome::OriginalMismatch : Outcome::ReplacedFromCache;
                result.detail = "Runtime proof result";
                if (!refuse) result.variant = bytes_of("runtime variant");
                return result;
            });
        const auto accepted = runtime_loader.prepare(request);
        require(accepted.outcome == Outcome::ReplacedFromCache && !accepted.variant.empty() && compiler.calls == 0,
            "production path delegates proof/cache/compile instead of reading offline trees");
        auto wrong = request;
        wrong.target = "ps_5_0";
        require(runtime_loader.prepare(wrong).outcome == Outcome::NotAllowlisted && delegated == 1,
            "stage eligibility applies before the Runtime delegate");
        wrong = request;
        wrong.source = bytes_of("unlisted Runtime root");
        require(runtime_loader.prepare(wrong).outcome == Outcome::NotAllowlisted && delegated == 1,
            "root eligibility applies before the Runtime delegate");
        refuse = true;
        const auto refused = runtime_loader.prepare(request);
        require(refused.outcome == Outcome::OriginalMismatch && refused.variant.empty() &&
            refused.detail == "Runtime proof result" && compiler.calls == 0, "Runtime refusals preserve stock fallback");
    }
    auto prepared = loader.prepare(request);
    require(prepared.outcome == Outcome::Replaced && compiler.calls == 2 && prepared.entry->family == Family::CoreStatic,
        "original reproduced, then the motion tree compiled");
    const std::string variant(reinterpret_cast<const char*>(prepared.variant.data()), prepared.variant.size());
    require(variant.find("|motion|" + vs_motion + "|main|vs_5_0|8|SMAP_size=2048;") != std::string::npos,
        "variant uses the motion root with the engine's exact inputs");
    require(std::filesystem::exists(root / "cache" / (loader.cache_key(request, *prepared.entry) + ".dxbc")), "verified variant cached");
    compiler.calls = 0;
    prepared = loader.prepare(request);
    require(prepared.outcome == Outcome::ReplacedFromCache && compiler.calls == 0, "cache hit skips compiles");
    auto other = request;
    other.defines.push_back({"SKIN_1", "1"});
    require(loader.cache_key(other, *prepared.entry) != loader.cache_key(request, *prepared.entry), "defines are part of the key");
    {
        std::ofstream(root / "cache" / (loader.cache_key(request, *prepared.entry) + ".dxbc"), std::ios::binary | std::ios::trunc) << "garbage";
        compiler.calls = 0;
        prepared = loader.prepare(request);
        require(prepared.outcome == Outcome::Replaced && compiler.calls == 2, "bad cache entry recompiles");
    }
    std::filesystem::remove_all(root / "cache");
    compiler.drift = "x";
    prepared = loader.prepare(request);
    require(prepared.outcome == Outcome::OriginalMismatch && prepared.variant.empty(), "engine bytes not reproduced: stock");
    compiler.drift.clear();
    compiler.fail_motion = true;
    require(loader.prepare(request).outcome == Outcome::MotionCompileFailed, "motion compile failure: stock");
    compiler.fail_motion = false;
    reflector.edit = [](char, bool variant, Reflection& r) { if (variant) r.buffers[0].size = 80; };
    prepared = loader.prepare(request);
    require(prepared.outcome == Outcome::ReflectionMismatch && prepared.compatibility == Compatibility::Buffer, "reflection mismatch: stock");
    reflector.edit = nullptr;
    auto wrong_stage = request;
    wrong_stage.target = "ps_5_0";
    require(loader.prepare(wrong_stage).outcome == Outcome::NotAllowlisted, "a VS root never matches a PS target");
    auto pixel = request;
    pixel.target = "ps_5_0";
    pixel.source = bytes_of(ps_source);
    compiler.compile(root / "original", pixel.source, pixel, engine_code, error);
    pixel.bytecode = engine_code;
    require(loader.prepare(pixel).outcome == Outcome::MotionCompileFailed, "motion root missing from the tree: stock");

    // Game deployments use an extended-length "\\?\" path, which does not normalise '/', while manifest
    // names such as "dx11/tess.hs" contain one. The motion root must still be found.
    {
        const std::string sub_source = "sub root source", sub_motion = "sub motion root";
        std::filesystem::create_directories(root / "motion" / "r3" / "dx11");
        std::ofstream(root / "motion" / "r3" / "dx11" / "deffer_base_flat_d.vs", std::ios::binary) << sub_motion;
        auto sub_manifest = Manifest::parse("rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\nroot vs core_static dx11/deffer_base_flat_d.vs " +
            sha(sub_source) + " " + sha(sub_motion) + "\n", error);
        require(sub_manifest.has_value(), "manifest accepts a subdirectory root: " + error);
        const std::filesystem::path extended = L"\\\\?\\" + std::filesystem::absolute(root).wstring();
        ShaderLoader extended_loader(*sub_manifest, extended, extended / "cache", compiler, reflector);
        auto sub = request;
        sub.source = bytes_of(sub_source);
        compiler.compile(root / "original", sub.source, sub, engine_code, error);
        sub.bytecode = engine_code;
        require(extended_loader.prepare(sub).outcome == Outcome::Replaced, "'/' in a manifest name under a \\\\?\\ deployment path");
    }
    std::filesystem::remove_all(root);
}

// ---- foliage (trees and grass) ---------------------------------------------------------------------------------
// One detail group as the engine writes it: rows 0..2 are the rotation columns times the effective scale plus the world
// translation, row 3 is (sun, sun, sun, hemisphere).
std::array<float, 16> detail_group(float tx, float ty, float tz, float scale, float sun, float hemi)
{
    return {scale, 0, 0, tx, 0, scale, 0, ty, 0, 0, scale, tz, sun, sun, sun, hemi};
}
void put_group(DetailRows& rows, std::size_t slot, const std::array<float, 16>& group)
{
    std::memcpy(rows.data() + slot * 16, group.data(), sizeof(float) * 16);
}
DetailRows empty_batch()
{
    DetailRows rows;
    poison_detail_rows(rows);
    return rows;
}
FoliageConstants wind(float time, float x)
{
    FoliageConstants c;
    const float consts[4] = {1.0f / 16384, 1.0f / 16384, 0.5f, 0.25f};
    const float wave[4] = {0.2f, 0.14f, 0.33f, time};
    const float dir[4] = {x, 0, 0.5f, 0};
    std::memcpy(c.consts, consts, sizeof(consts));
    std::memcpy(c.wave, wave, sizeof(wave));
    std::memcpy(c.wind, dir, sizeof(dir));
    return c;
}
bool same(const float* a, const float* b, std::size_t count) { return std::memcmp(a, b, count * sizeof(float)) == 0; }

void foliage_layout_readme()
{
    const std::filesystem::path root = RSF_SOURCE_DIR;
    const auto readme = read_text(root / "games/anomaly/shaders/foliage/README.md");
    const auto header = read_text(root / "games/anomaly/shaders/foliage/rsf_anomaly_foliage.h");
    const auto row = [&](const std::string& pattern) {
        std::smatch m;
        require(std::regex_search(readme, m, std::regex(pattern)), "README row not found: " + pattern);
        return std::stoul(m[1]);
    };
    require(row(R"(\| (\d+) \| row_major float4x4 rsf_previous_clip_from_world \| 64)") == foliage_clip_offset, "clip offset");
    require(row(R"(\| (\d+) \| row_major float3x4 rsf_previous_tree_xform \| 48)") == foliage_tree_xform_offset, "xform offset");
    require(row(R"(\| (\d+) \| float4 rsf_previous_consts \| 16)") == foliage_consts_offset, "consts offset");
    require(row(R"(\| (\d+) \| float4 rsf_previous_wave \| 16)") == foliage_wave_offset, "wave offset");
    require(row(R"(\| (\d+) \| float4 rsf_previous_wind, tree wind / detail dir2D \| 16)") == foliage_wind_offset, "wind offset");
    require(row(R"(\| (\d+) \| uint4 rsf_foliage_history \| 16)") == foliage_history_offset, "history offset");
    require(row(R"(\| (\d+) \| float4 rsf_previous_rows\[244\] \| 3904)") == foliage_rows_offset, "rows offset");
    require(row(R"(\| (\d+) \| float4 rsf_previous_item_state\[61\] \| 976)") == foliage_item_state_offset, "item state offset");
    require(foliage_rows_offset + foliage_detail_rows * 16 == foliage_item_state_offset &&
        foliage_item_state_offset + foliage_detail_groups * 16 == foliage_vs_bytes, "tight tail");
    require(header.find("register(b13)") != std::string::npos && header.find("rsf_previous_rows[61 * 4]") != std::string::npos &&
        header.find("rsf_previous_item_state[61]") != std::string::npos, "HLSL header shape");
    require(foliage_detail_groups == 61 && foliage_detail_rows == 244 && foliage_detail_floats == 976, "detail dimensions");
}

void foliage_layout_from_reflection()
{
    // A tree VS: m_xform float3x4 (48 bytes, three registers), wave/wind/consts float4; the engine indexes constant
    // buffers by the reflection's order.
    Reflection tree;
    tree.buffers = {{"dynamic_transforms", 256, {{"m_VP", 0, 64}}},
        {"$Globals", 144, {{"m_xform", 0, 48}, {"consts", 48, 16}, {"c_scale", 64, 16}, {"wind", 80, 16}, {"wave", 96, 16}}}};
    auto layout = make_foliage_layout(Family::FoliageTree, tree);
    require(layout.usable && layout.xform.buffer == 1 && layout.xform.offset == 0 && layout.xform.size == 48 &&
        layout.xform.buffer_size == 144 && layout.wave.offset == 96 && layout.wind.offset == 80 && layout.consts.offset == 48,
        "tree layout");
    // USE_TREEWAVE: wave, wind and consts may vanish; the tree is still usable.
    tree.buffers[1].variables = {{"m_xform", 0, 48}};
    layout = make_foliage_layout(Family::FoliageTree, tree);
    require(layout.usable && !layout.wave.present && !layout.wind.present && !layout.consts.present, "removed variables are absent");
    tree.buffers[1].variables = {{"consts", 48, 16}};
    require(!make_foliage_layout(Family::FoliageTree, tree).usable, "no xform is unusable");
    tree.buffers[1].variables = {{"m_xform", 120, 48}};
    require(!make_foliage_layout(Family::FoliageTree, tree).usable, "variable past its buffer is unusable");

    Reflection detail;
    detail.buffers = {{"$Globals", 4048, {{"consts", 0, 16}, {"wave", 16, 16}, {"dir2D", 32, 16}, {"array", 48, 3904}}}};
    layout = make_foliage_layout(Family::FoliageDetail, detail);
    require(layout.usable && layout.array.offset == 48 && layout.array.size == 3904 && layout.wind.offset == 32 &&
        layout.wave.present, "detail wave layout");
    detail.buffers[0].variables = {{"consts", 0, 16}, {"array", 16, 3904}};   // the still route has no wave or dir2D
    layout = make_foliage_layout(Family::FoliageDetail, detail);
    require(layout.usable && !layout.wave.present && !layout.wind.present, "detail still layout");
    detail.buffers[0].variables = {{"array", 16, 3000}};
    require(!make_foliage_layout(Family::FoliageDetail, detail).usable, "a short array is unusable");
    require(!make_foliage_layout(Family::CoreStatic, detail).usable, "core family has no foliage layout");
}

void foliage_packing()
{
    const auto clip = sample_matrix(0.5);
    const float xform[12] = {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12};
    const auto previous = wind(3.5f, 0.75f);
    FoliageVertexBytes bytes;
    require(pack_foliage_tree(clip, xform, previous, bytes), "tree pack");
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c)
            require(f32(bytes.data(), foliage_clip_offset + (r * 4 + c) * 4) == static_cast<float>(clip.values[r * 4 + c]), "clip rows");
    for (std::size_t i = 0; i < 12; ++i) require(f32(bytes.data(), foliage_tree_xform_offset + i * 4) == xform[i], "xform registers verbatim");
    require(same(reinterpret_cast<const float*>(bytes.data() + foliage_consts_offset), previous.consts, 4) &&
        same(reinterpret_cast<const float*>(bytes.data() + foliage_wave_offset), previous.wave, 4) &&
        same(reinterpret_cast<const float*>(bytes.data() + foliage_wind_offset), previous.wind, 4), "consts, wave, wind");
    std::uint32_t history[4];
    std::memcpy(history, bytes.data() + foliage_history_offset, 16);
    require(history[0] == 1 && history[1] == 0 && history[2] == 1 && history[3] == 0, "tree history uint4 (complete, 0, ABI 1, 0)");
    for (std::size_t i = foliage_rows_offset; i < foliage_vs_bytes; ++i) require(bytes[i] == std::byte{}, "tree has no rows or item state");
    float bad_xform[12];
    std::memcpy(bad_xform, xform, sizeof(xform));
    bad_xform[5] = std::nanf("");
    require(!pack_foliage_tree(clip, bad_xform, previous, bytes), "NaN xform refused");
    std::memcpy(history, bytes.data() + foliage_history_offset, 16);
    require(history[0] == 0 && history[2] == 0, "a refusal leaves the all-zero invalid buffer");
    auto bad_wind = previous;
    bad_wind.wind[2] = INFINITY;
    require(!pack_foliage_tree(clip, xform, bad_wind, bytes), "infinite wind refused");

    // Detail: only the matched groups carry rows and a state.
    std::array<FoliageItem, foliage_detail_groups> items{};
    const auto g1 = detail_group(10, 20, 30, 0.5f, 0.3f, 0.7f), g3 = detail_group(-4, 5, 6, 1.5f, 0.2f, 0.9f);
    items[1] = {true, 0.5f, 1.0f, {}};
    std::memcpy(items[1].rows, g1.data(), sizeof(float) * 16);
    items[3] = {true, 1.5f, 0.5f, {}};
    std::memcpy(items[3].rows, g3.data(), sizeof(float) * 16);
    require(pack_foliage_detail(clip, previous, items, 5, bytes), "detail pack");
    std::memcpy(history, bytes.data() + foliage_history_offset, 16);
    require(history[0] == 1 && history[1] == 5 && history[2] == 1 && history[3] == 0, "detail history (complete, live groups, ABI 1, 0)");
    require(same(reinterpret_cast<const float*>(bytes.data() + foliage_rows_offset + 1 * 64), g1.data(), 16), "group 1 rows");
    require(same(reinterpret_cast<const float*>(bytes.data() + foliage_rows_offset + 3 * 64), g3.data(), 16), "group 3 rows");
    require(f32(bytes.data(), foliage_item_state_offset + 1 * 16) == 1 && f32(bytes.data(), foliage_item_state_offset + 1 * 16 + 4) == 0.5f &&
        f32(bytes.data(), foliage_item_state_offset + 1 * 16 + 8) == 1.0f && f32(bytes.data(), foliage_item_state_offset + 1 * 16 + 12) == 0,
        "state (1, scale, fade, 0)");
    require(f32(bytes.data(), foliage_item_state_offset + 0 * 16) == 0 && f32(bytes.data(), foliage_rows_offset) == 0, "unmatched group 0 is zero");
    require(f32(bytes.data(), foliage_item_state_offset + 4 * 16) == 0, "unmatched group 4 is zero");
    items[3].fade = 1.5f;   // outside (0, 1]
    require(pack_foliage_detail(clip, previous, items, 5, bytes) && f32(bytes.data(), foliage_item_state_offset + 3 * 16) == 0 &&
        f32(bytes.data(), foliage_rows_offset + 3 * 64) == 0, "a state the shader would reject is not bound");
    require(!pack_foliage_detail(clip, previous, items, 62, bytes), "more than 61 groups refused");
    // Jitter: removing it from a jittered projection restores the unjittered one (column vectors, NDC (2u, -2v)).
    temporal::Mat4 p;
    p.values = {1.2, 0, 0, 0, 0, 2.1, 0, 0, 0, 0, 1.001, -0.1, 0, 0, 1, 0};
    const float ju = 0.003f, jv = -0.002f;
    auto jittered = p;
    for (std::size_t c = 0; c < 4; ++c) {
        jittered.values[0 * 4 + c] += 2.0 * ju * p.values[3 * 4 + c];
        jittered.values[1 * 4 + c] -= 2.0 * jv * p.values[3 * 4 + c];
    }
    const auto restored = remove_jitter(jittered, ju, jv);
    for (std::size_t i = 0; i < 16; ++i) require(std::abs(restored.values[i] - p.values[i]) < 1e-12, "jitter removed");
    require(remove_jitter(p, 0, 0) == p, "zero jitter is the identity");
}

void foliage_identity_remap()
{
    using temporal::ViewId;
    FoliageHistory h;
    const std::uint64_t route = 0x1111, other_route = 0x2222;
    const auto w0 = wind(1.0f, 0.5f), w1 = wind(1.1f, 0.6f);
    // Frame 10: two batches of one route. Items are identified by what they are, not by where the engine put them.
    require(!h.begin_frame(ViewId::Main, 10, 0, 640, 360), "first frame has no previous");
    DetailRows batch = empty_batch();
    put_group(batch, 0, detail_group(1, 0, 1, 1.0f, 0.1f, 0.5f));
    put_group(batch, 1, detail_group(2, 0, 2, 0.9f, 0.1f, 0.5f));
    put_group(batch, 2, detail_group(3, 0, 3, 0.8f, 0.2f, 0.6f));
    auto remap = h.record_detail(ViewId::Main, route, w0, batch);
    require(remap.live == 3 && remap.matched == 0 && remap.unmatched == 3 && !remap.route_found, "frame one: nothing to remap");
    DetailRows second = empty_batch();
    put_group(second, 0, detail_group(4, 0, 4, 0.7f, 0.2f, 0.6f));
    put_group(second, 1, detail_group(5, 0, 5, 0.6f, 0.3f, 0.7f));
    h.record_detail(ViewId::Main, route, w0, second);
    require(h.recorded_items(ViewId::Main) == 5, "five instances recorded, the poisoned tails are not");

    // Frame 11: the same plants land in other batches and other groups (the camera moved, items culled and added), the
    // wind and fade changed. Previous rows are bound to the CURRENT group of the same plant.
    require(h.begin_frame(ViewId::Main, 11, 0, 640, 360), "continuous frame");
    DetailRows now = empty_batch();
    put_group(now, 0, detail_group(5, 0, 5, 0.65f, 0.3f, 0.7f));   // was batch 2 group 1
    put_group(now, 1, detail_group(9, 0, 9, 1.0f, 0.4f, 0.4f));    // new plant: no history
    put_group(now, 2, detail_group(1, 0, 1, 0.95f, 0.1f, 0.5f));   // was batch 1 group 0
    put_group(now, 3, detail_group(3, 0, 3, 0.75f, 0.2f, 0.6f));   // was batch 1 group 2
    remap = h.record_detail(ViewId::Main, route, w1, now);
    require(remap.live == 4 && remap.route_found && remap.matched == 3 && remap.unmatched == 1, "three of four remapped");
    require(remap.previous == w0, "previous consts, wave and wind of the route");
    const auto p5 = detail_group(5, 0, 5, 0.6f, 0.3f, 0.7f), p1 = detail_group(1, 0, 1, 1.0f, 0.1f, 0.5f),
               p3 = detail_group(3, 0, 3, 0.8f, 0.2f, 0.6f);
    require(remap.items[0].valid && same(remap.items[0].rows, p5.data(), 16) && remap.items[0].scale == 0.6f, "group 0 gets plant 5's previous rows");
    require(!remap.items[1].valid, "a new plant has no history");
    require(remap.items[2].valid && same(remap.items[2].rows, p1.data(), 16) && remap.items[2].scale == 1.0f, "group 2 gets plant 1's previous rows");
    require(remap.items[3].valid && same(remap.items[3].rows, p3.data(), 16), "group 3 gets plant 3's previous rows");
    require(remap.items[2].fade == 1.0f && !remap.items[4].valid, "fade gate, groups past the batch are invalid");
    // Pack it: the shader sees exactly those rows at the current groups.
    FoliageVertexBytes bytes;
    require(pack_foliage_detail(compose(sample_matrix(0.2), sample_matrix(0.1)), remap.previous, remap.items, remap.live, bytes), "pack");
    require(same(reinterpret_cast<const float*>(bytes.data() + foliage_rows_offset + 2 * 64), p1.data(), 16) &&
        f32(bytes.data(), foliage_item_state_offset + 1 * 16) == 0, "bound rows follow the current slots");

    // Frame 12: a plant that changed route (nearer, now the other wave set) must not borrow the other route's constants;
    // an unknown route has no previous at all; a repeated identical group is harmless, a contradictory one a conflict.
    require(h.begin_frame(ViewId::Main, 12, 0, 640, 360), "frame 12");
    DetailRows moved = empty_batch();
    put_group(moved, 0, detail_group(5, 0, 5, 0.66f, 0.3f, 0.7f));
    DetailRows foreign = empty_batch();
    put_group(foreign, 0, detail_group(50, 0, 50, 1.0f, 0.1f, 0.1f));
    remap = h.record_detail(ViewId::Main, other_route, w1, foreign);
    require(!remap.route_found && remap.matched == 0, "a route not drawn last frame has no history");
    remap = h.record_detail(ViewId::Main, route, w1, moved);
    require(remap.route_found && remap.matched == 1, "same plant, same route matches");
    DetailRows again = moved;
    require(h.record_detail(ViewId::Main, route, w1, again).conflicts == 0, "an identical repeat (second pass) is not a conflict");
    put_group(again, 0, detail_group(5, 0, 5, 0.5f, 0.3f, 0.7f));
    require(h.record_detail(ViewId::Main, route, w1, again).conflicts == 1 && h.conflicts() == 1, "contradicting repeat is a conflict");
    // That plant now has no usable history for the next frame; a route whose constants differ in one frame has none either.
    require(h.begin_frame(ViewId::Main, 13, 0, 640, 360), "frame 13");
    remap = h.record_detail(ViewId::Main, route, w1, moved);
    require(remap.route_found && remap.matched == 0, "the conflicted plant is not history");
    remap = h.record_detail(ViewId::Main, other_route, w1, foreign);
    require(remap.route_found && remap.matched == 1, "the other route and its plant were drawn in frame 12");
    DetailRows tail = empty_batch();
    require(h.record_detail(ViewId::Main, route, w1, tail).live == 0, "a fully poisoned batch has no live groups");
    DetailRows garbage = empty_batch();
    put_group(garbage, 0, detail_group(7, 0, 7, 1.0f, 0.1f, 0.1f));
    garbage[16 * 1 + 5] = std::nanf("");   // a half-written second group
    require(count_live_groups(garbage) == 1, "a non-finite group ends the batch");
    require(detail_group_scale(detail_group(0, 0, 0, 0.75f, 0, 0).data()) == 0.75f, "effective scale from the first row");

    // Frame rules shared with the core history.
    require(h.begin_frame(ViewId::Main, 14, 0, 640, 360), "frame 14");
    require(h.record_detail(ViewId::Main, route, w1, moved).route_found, "history steady");
    require(!h.begin_frame(ViewId::Main, 15, 0, 1280, 720), "a resize drops history");
    require(!h.record_detail(ViewId::Main, route, w1, moved).route_found, "resized: no previous");
    require(h.begin_frame(ViewId::Main, 16, 0, 1280, 720), "frame 16");
    require(!h.begin_frame(ViewId::Main, 17, 1, 1280, 720), "a reset generation drops history");
    require(h.begin_frame(ViewId::Main, 18, 1, 1280, 720) && h.begin_frame(ViewId::Main, 18, 1, 1280, 720), "split scene re-open keeps it");
    require(!h.begin_frame(ViewId::Main, 3, 1, 1280, 720), "an older frame number restarts history");
    require(!h.active(ViewId::Scope), "scope view independent");
    h.begin_frame(ViewId::Scope, 20, 0, 320, 180);
    require(h.record_detail(ViewId::Scope, route, w1, moved).live == 1 && !h.record_detail(ViewId::Scope, route, w1, moved).route_found,
        "scope has its own frames");
    h.reset_all();
    require(!h.active(ViewId::Main), "reset forgets everything");

    // Trees: the visual's address, its xform bytes and its slide window.
    TreeRecord tree;
    const float xform[12] = {1, 0, 0, 5, 0, 1, 0, 6, 0, 0, 1, 7};
    std::memcpy(tree.xform, xform, sizeof(xform));
    tree.constants = w0;
    tree.lod = 3;
    h.begin_frame(ViewId::Main, 30, 0, 640, 360);
    require(h.record_tree(ViewId::Main, 0xabc0, tree) == nullptr, "first sight of a tree");
    require(h.begin_frame(ViewId::Main, 31, 0, 640, 360), "frame 31");
    auto tree_now = tree;
    tree_now.constants = w1;
    const auto* before = h.record_tree(ViewId::Main, 0xabc0, tree_now);
    require(before && *before == tree && before->constants == w0, "the same visual's previous record (previous wind and time)");
    require(h.record_tree(ViewId::Main, 0xabc0, tree_now) == before, "identical repeat");
    auto contradicting = tree_now;
    contradicting.lod = 4;
    const auto conflicts_before = h.conflicts();
    require(h.record_tree(ViewId::Main, 0xabc0, contradicting) == nullptr && h.conflicts() == conflicts_before + 1, "same-frame contradiction");
    require(h.record_tree(ViewId::Main, 0xdef0, tree_now) == nullptr, "another tree has its own history");
    require(h.begin_frame(ViewId::Main, 32, 0, 640, 360), "frame 32");
    require(h.record_tree(ViewId::Main, 0xabc0, tree_now) == nullptr, "a conflicted tree is not history");
}

void foliage_policy()
{
    using geometry::DrawKind;
    using geometry::DrawVerdict;
    CameraSample camera{sample_matrix(0.1), sample_matrix(0.2)};
    TreeRecord previous;
    const float xform[12] = {1, 0, 0, 5, 0, 1, 0, 6, 0, 0, 1, 7};
    std::memcpy(previous.xform, xform, sizeof(xform));
    previous.constants = wind(1.0f, 0.5f);
    previous.lod = 2;
    TreeDrawInput tree;
    tree.family = Family::FoliageTree;
    tree.in_visual = true;
    tree.kind = DrawKind::Tree;
    tree.verdict = DrawVerdict::IdentifiedFoliage;
    tree.context = true;
    tree.previous_camera = &camera;
    tree.constants_ok = true;
    tree.current = previous;
    tree.current.constants = wind(1.1f, 0.6f);
    tree.previous = &previous;
    auto d = decide_tree(tree);
    require(d.reason == HistoryReason::Valid && d.previous_clip == compose(camera.projection, camera.view), "tree valid: previous P*V");
    auto t = tree; t.family = Family::CoreStatic;
    require(decide_tree(t).reason == HistoryReason::KindMismatch, "core VS on a tree draw");
    t = tree; t.in_visual = false;
    require(decide_tree(t).reason == HistoryReason::NoVisual, "outside a visual");
    t = tree; t.verdict = DrawVerdict::UnidentifiedDeformed;
    require(decide_tree(t).reason == HistoryReason::Unidentified, "not identified");
    t = tree; t.context = false;
    require(decide_tree(t).reason == HistoryReason::NoContext, "no context");
    t = tree; t.constants_ok = false;
    require(decide_tree(t).reason == HistoryReason::FoliageNoConstants, "constants unreadable");
    t = tree; t.camera_conflict = true;
    require(decide_tree(t).reason == HistoryReason::CameraConflict, "camera conflict");
    t = tree; t.previous_camera = nullptr;
    require(decide_tree(t).reason == HistoryReason::NoPreviousCamera, "first frame");
    t = tree; t.previous = nullptr;
    require(decide_tree(t).reason == HistoryReason::FoliageNoPrevious, "tree not drawn last frame");
    t = tree; t.current.lod = 3;
    require(decide_tree(t).reason == HistoryReason::FoliageLodChanged, "slide window changed");
    t = tree; t.current.xform[3] = 9;
    require(decide_tree(t).reason == HistoryReason::FoliageXformChanged, "a recycled address with another xform");
    t = tree; t.previous_jitter_u = 0.004f; t.previous_jitter_v = -0.001f;
    auto jittered = camera;
    for (std::size_t c = 0; c < 4; ++c) {
        jittered.projection.values[0 * 4 + c] += 2.0 * 0.004 * camera.projection.values[3 * 4 + c];
        jittered.projection.values[1 * 4 + c] -= 2.0 * -0.001 * camera.projection.values[3 * 4 + c];
    }
    t.previous_camera = &jittered;
    d = decide_tree(t);
    require(d.reason == HistoryReason::Valid && d.previous_clip == compose(remove_jitter(jittered.projection, 0.004f, -0.001f), camera.view),
        "the previous projection is unjittered for foliage");

    DetailDrawInput detail;
    detail.family = Family::FoliageDetail;
    detail.detail_window = true;
    detail.context = true;
    detail.previous_camera = &camera;
    detail.constants_ok = true;
    detail.world_readable = true;
    detail.world_identity = true;
    detail.route_found = true;
    detail.live_groups = 12;
    auto dd = decide_detail(detail);
    require(dd.reason == HistoryReason::Valid && dd.previous_clip == compose(camera.projection, camera.view), "detail valid");
    auto x = detail; x.family = Family::FoliageTree;
    require(decide_detail(x).reason == HistoryReason::KindMismatch, "tree VS on a detail draw");
    x = detail; x.detail_window = false;
    require(decide_detail(x).reason == HistoryReason::NoVisual, "outside CDetailManager::Render");
    x = detail; x.context = false;
    require(decide_detail(x).reason == HistoryReason::NoContext, "no context");
    x = detail; x.live_groups = 0;
    require(decide_detail(x).reason == HistoryReason::FoliageNoConstants, "no live instances");
    x = detail; x.world_identity = false;
    require(decide_detail(x).reason == HistoryReason::FoliageWorld, "the transform assumes an identity world");
    x = detail; x.world_readable = false;
    require(decide_detail(x).reason == HistoryReason::FoliageWorld, "unreadable world");
    x = detail; x.camera_conflict = true;
    require(decide_detail(x).reason == HistoryReason::CameraConflict, "camera conflict");
    x = detail; x.previous_camera = nullptr;
    require(decide_detail(x).reason == HistoryReason::NoPreviousCamera, "first frame");
    x = detail; x.route_found = false;
    require(decide_detail(x).reason == HistoryReason::FoliageNoPrevious, "route not drawn last frame");
    require(std::string(history_reason_name(HistoryReason::FoliageXformChanged)) == "foliage_xform_changed" &&
        static_cast<std::size_t>(HistoryReason::Count) == 16, "reason names and counter width (MotionCounters::reasons)");
}

// ---- tessellation chain (DX11\tess.hs / DX11\tess.ds) ----------------------------------------------------------
void tess_layout_and_packing()
{
    const auto tess = read_text(std::filesystem::path(RSF_SOURCE_DIR) / "games/anomaly/shaders/core/rsf_core_tess.h");
    const auto row = [&](const std::string& pattern) {
        std::smatch m;
        require(std::regex_search(tess, m, std::regex(pattern)), "rsf_core_tess.h: " + pattern);
        return std::stoul(m[1]);
    };
    require(row(R"(rsf_previous_wv : packoffset\(c(\d+)\))") * 16 == tess_wv_offset, "tess wv packoffset");
    require(row(R"(rsf_previous_projection : packoffset\(c(\d+)\))") * 16 == tess_projection_offset, "tess projection packoffset");
    require(row(R"(rsf_tess_history : packoffset\(c(\d+)\))") * 16 == tess_history_offset, "tess history packoffset");
    require(tess_history_offset + 16 == tess_ds_bytes && tess.find("RSFTessPrevious : register(b13)") != std::string::npos,
        "DS b13 size and slot");
    TessDomainBytes bytes;
    const auto wv = sample_matrix(0.5), projection = sample_matrix(1.5);
    require(pack_tess_domain(wv, projection, true, bytes), "pack");
    for (std::size_t r = 0; r < 4; ++r)
        for (std::size_t c = 0; c < 4; ++c) {
            require(f32(bytes.data(), tess_wv_offset + (r * 4 + c) * 4) == static_cast<float>(wv.values[r * 4 + c]), "wv rows");
            require(f32(bytes.data(), tess_projection_offset + (r * 4 + c) * 4) == static_cast<float>(projection.values[r * 4 + c]),
                "projection rows");
        }
    require(f32(bytes.data(), tess_history_offset) == 1 && f32(bytes.data(), tess_history_offset + 4) == 0 &&
        f32(bytes.data(), tess_history_offset + 8) == 0 && f32(bytes.data(), tess_history_offset + 12) == 1, "history (1, 0, 0, ABI 1)");
    require(!pack_tess_domain(wv, projection, false, bytes), "no history refused");
    require(f32(bytes.data(), tess_history_offset) == 0 && f32(bytes.data(), tess_history_offset + 12) == 1 &&
        f32(bytes.data(), tess_wv_offset) == 0, "refusal writes the invalid buffer");
    auto bad = projection;
    bad.values[10] = std::nan("");
    require(!pack_tess_domain(wv, bad, true, bytes) && f32(bytes.data(), tess_history_offset) == 0, "NaN projection refused");
    bytes.fill(std::byte{0x7f});
    pack_tess_invalid(bytes);
    for (std::size_t i = 0; i < tess_history_offset + 12; i += 4) require(f32(bytes.data(), i) == 0, "invalid zero");
}

// decide_core's split for the domain stage multiplies back to the VS's previous WVP.
void tess_policy_split()
{
    using geometry::DrawKind;
    using geometry::DrawVerdict;
    CameraSample camera{sample_matrix(0.1), sample_matrix(0.2)};
    temporal::InstanceSnapshot previous;
    previous.world = sample_matrix(0.7);
    previous.bones = {sample_matrix(1), sample_matrix(2)};
    CoreDrawInput in{Family::CoreModel, 2, true, DrawVerdict::Identified, DrawKind::HardwareSkinned, true, false, &camera, &previous};
    const auto d = decide_core(in);
    require(d.reason == HistoryReason::Valid, "skinned history valid");
    const auto product = compose(d.previous_projection, d.previous_wv);
    for (std::size_t i = 0; i < 16; ++i)
        require(std::abs(product.values[i] - d.previous_wvp.values[i]) <= 1e-9 * (1 + std::abs(d.previous_wvp.values[i])),
            "previous_projection * previous_wv == previous_wvp");
    require(d.previous_projection == camera.projection && d.previous_wv == compose(camera.view, previous.world), "split factors");
    // SKIN_0: the bone-composed previous world is inside previous_wv.
    previous.bones.clear();
    in.skin = 0;
    in.kind = DrawKind::SingleBone;
    const auto single = decide_core(in);
    require(single.reason == HistoryReason::Valid && single.previous_wv == compose(camera.view, previous.world), "single bone split");
}

// Control-point interfaces: RSF fields first (registers of the native fields shift), TEXCOORD14/15 before SV_Position.
Parameter p(const char* name, std::uint32_t index, std::uint32_t reg, std::uint32_t mask, std::uint32_t sv = 0)
{
    return {name, index, reg, sv, 3, mask};
}
std::vector<Parameter> stock_control() { return {p("TEXCOORD", 0, 0, 0x3), p("TEXCOORD", 1, 1, 0xf), p("TEXCOORD", 2, 2, 0x7)}; }
std::vector<Parameter> rsf_control()
{
    return {p("TEXCOORD", 12, 0, 0xf), p("TEXCOORD", 13, 1, 0x1), p("TEXCOORD", 0, 2, 0x3), p("TEXCOORD", 1, 3, 0xf),
        p("TEXCOORD", 2, 4, 0x7), p("TEXCOORD", 14, 5, 0xf), p("TEXCOORD", 15, 6, 0x7)};
}
std::vector<Parameter> patch_constants()
{
    return {p("SV_TessFactor", 0, 0, 0x1, 13), p("POSITION", 4, 0, 0xe), p("SV_InsideTessFactor", 0, 1, 0x1, 14),
        p("POSITION", 3, 2, 0x7), p("NORMAL", 3, 3, 0x7)};
}
Reflection tess_stock(char stage)
{
    Reflection r;
    r.version = stage == 'h' ? 0x30050 : 0x40050;
    r.bindings = {{"$Globals", 0, 0, 1, 0, 0}};
    r.buffers = {{"$Globals", 32, {{stage == 'h' ? "triLOD" : "m_P", 0, 16}}}};
    r.inputs = stock_control();
    r.outputs = stage == 'h' ? stock_control()
                             : std::vector<Parameter>{p("TEXCOORD", 0, 0, 0x3), p("TEXCOORD", 1, 1, 0xf), p("SV_Position", 0, 2, 0xf, 1)};
    r.patch = patch_constants();
    return r;
}
Reflection tess_variant(char stage)
{
    auto r = tess_stock(stage);
    r.inputs = rsf_control();
    if (stage == 'h') {
        r.outputs = rsf_control();
    } else {
        r.outputs = {p("TEXCOORD", 12, 0, 0xf), p("TEXCOORD", 13, 1, 0x1), p("TEXCOORD", 0, 2, 0x3), p("TEXCOORD", 1, 3, 0xf),
            p("SV_Position", 0, 4, 0xf, 1)};
        r.bindings.push_back({"RSFTessPrevious", 0, 13, 1, 0, 0});
    }
    return r;
}
void tess_compatibility_rules()
{
    for (const char stage : {'h', 'd'}) {
        const auto stock = tess_stock(stage);
        const auto variant = tess_variant(stage);
        const std::string s(1, stage);
        require(compatible(stage, stock, variant) == Compatibility::Ok, s + ": reference variant compatible");
        auto v = variant; v.patch[1].mask = 0xf;
        require(compatible(stage, stock, v) == Compatibility::PatchConstants, s + ": patch-constant signature must be identical");
        v = variant; v.inputs.pop_back();
        require(compatible(stage, stock, v) == Compatibility::Inputs, s + ": missing TEXCOORD15");
        v = variant; v.inputs.push_back(p("TEXCOORD", 16, 7, 0xf));
        require(compatible(stage, stock, v) == Compatibility::Inputs, s + ": unknown extra input");
        v = variant; v.inputs[0].mask = 0x7;
        require(compatible(stage, stock, v) == Compatibility::Inputs, s + ": TEXCOORD12 must be xyzw");
        v = variant; v.inputs.erase(v.inputs.begin() + 3);
        require(compatible(stage, stock, v) == Compatibility::Inputs, s + ": lost a native control-point field");
        v = variant; v.buffers[0].variables[0].offset = 16;
        require(compatible(stage, stock, v) == Compatibility::Buffer, s + ": moved engine constant");
    }
    auto hull = tess_variant('h');
    hull.bindings.push_back({"RSFTessPrevious", 0, 13, 1, 0, 0});
    require(compatible('h', tess_stock('h'), hull) == Compatibility::ExtraBinding, "the hull stage binds no RSF buffer");
    auto domain = tess_variant('d');
    domain.bindings.back().name = "RSFCorePrevious";
    require(compatible('d', tess_stock('d'), domain) == Compatibility::ExtraBinding, "the domain stage binds only RSFTessPrevious");
    domain = tess_variant('d');
    domain.bindings.back().point = 12;
    require(compatible('d', tess_stock('d'), domain) == Compatibility::ExtraBinding, "RSFTessPrevious at b13 only");
    domain = tess_variant('d');
    domain.outputs.push_back(p("TEXCOORD", 14, 5, 0xf));
    require(compatible('d', tess_stock('d'), domain) == Compatibility::Outputs, "the domain stage hands the PS only TEXCOORD12/13");
    domain = tess_variant('d');
    domain.outputs.erase(domain.outputs.begin() + 1);
    require(compatible('d', tess_stock('d'), domain) == Compatibility::Outputs, "domain must write validity");
    hull = tess_variant('h');
    hull.outputs.erase(hull.outputs.begin());
    require(compatible('h', tess_stock('h'), hull) == Compatibility::Outputs, "hull must forward the previous clip");
    require(compatible('g', tess_stock('h'), tess_variant('h')) != Compatibility::Ok, "geometry shaders are never variants");
}
void tess_linkage_rules()
{
    Reflection vs;
    vs.outputs = rsf_control();
    vs.outputs.push_back(p("SV_Position", 0, 7, 0xf, 1));
    const auto hs = tess_variant('h'), ds = tess_variant('d');
    Reflection ps;
    ps.inputs = {p("TEXCOORD", 12, 0, 0xf), p("TEXCOORD", 13, 1, 0x1), p("TEXCOORD", 0, 2, 0x3), p("TEXCOORD", 1, 3, 0xf),
        p("SV_Position", 0, 4, 0x3, 1)};
    require(tess_chain_linkable(vs, hs, ds, ps), "variant chain links");
    require(!tess_chain_linkable(vs, tess_stock('h'), ds, ps), "stock hull refused");
    require(!tess_chain_linkable(vs, hs, tess_stock('d'), ps), "stock domain refused");
    auto plain = vs;   // a non-tessellation VS variant: no TEXCOORD14/15
    plain.outputs.erase(plain.outputs.begin() + 5, plain.outputs.begin() + 7);
    require(!tess_chain_linkable(plain, hs, ds, ps), "VS without the previous control data refused");
    auto shifted = hs;
    shifted.inputs[5].reg = 6;
    require(!tess_chain_linkable(vs, shifted, ds, ps), "register mismatch between VS output and HS input refused");
    auto unmatched = ds;
    unmatched.patch.push_back(p("NORMAL", 4, 4, 0x7));
    require(!tess_chain_linkable(vs, hs, unmatched, ps), "domain patch constant the hull does not write refused");
    unmatched = ds;
    unmatched.patch.clear();
    require(!tess_chain_linkable(vs, hs, unmatched, ps), "a domain without patch constants refused");
    require(!linkable(vs, ps), "the VS alone does not feed the PS under tessellation (SV_Position register)");
    require(family_stage(Family::TessHull) == 'h' && family_stage(Family::TessDomain) == 'd' &&
        std::string(family_name(Family::TessDomain)) == "tess_domain" && static_cast<std::size_t>(Family::Count) == 8 &&
        std::string(compatibility_name(Compatibility::PatchConstants)) == "patch_constants", "families and names");
}
void tess_manifest_rules()
{
    const std::string ns(64, 'a'), h1(64, '1'), h2(64, '2'), h3(64, '3'), h4(64, '4');
    const std::string head = "rsf-anomaly-motion-manifest 1\nnamespace " + ns + "\n";
    std::string error;
    auto ok = Manifest::parse(head + "root hs tess_hull dx11/tess.hs " + h1 + " " + h2 + "\nroot ds tess_domain dx11/tess.ds " + h3 + " " +
        h4 + "\n", error);
    require(ok && ok->by_original.at(h1).family == Family::TessHull && ok->by_original.at(h1).stage == 'h' &&
        ok->by_original.at(h3).stage == 'd' && ok->by_original.at(h3).name == "dx11/tess.ds", "hull/domain roots parse: " + error);
    for (const std::string bad : {"root vs tess_hull dx11/tess.hs ", "root ds tess_hull dx11/tess.hs ", "root gs tess_hull dx11/tess.hs ",
             "root hs tess_hull /tess.hs ", "root hs tess_hull dx11/ ", "root hs tess_hull a/b/tess.hs ", "root hs tess_hull dx11\\tess.hs ",
             "root hs tess_hull dx11/../tess.hs "})
        require(!Manifest::parse(head + bad + h1 + " " + h2 + "\n", error), "refused: " + bad);
}

// ---- far characters (docs/research/anomaly-far-character-motion.md) ---------------------------------------------
void far_character_diagnostics()
{
    // Bands: <20, 20-50, 50-100, 100-150, >=150 m, unknown (non-finite or negative).
    require(distance_band(0) == 0 && distance_band(19.99) == 0 && distance_band(20) == 1 && distance_band(49.9) == 1 &&
        distance_band(50) == 2 && distance_band(99.9) == 2 && distance_band(100) == 3 && distance_band(149.9) == 3 &&
        distance_band(150) == 4 && distance_band(5000) == 4, "distance bands");
    require(distance_band(std::nan("")) == distance_bands - 1 && distance_band(-1) == distance_bands - 1, "unknown band");
    require(std::string(distance_band_name(2)) == "50-100m" && std::string(distance_band_name(distance_bands - 1)) == "unknown" &&
        std::string(distance_band_name(99)) == "unknown", "band names");
    // Camera at (10, 2, -5) turned 90 degrees about Y; an object 120 m east of the camera.
    temporal::Mat4 rotation = temporal::Mat4::identity();
    rotation.values[0] = 0; rotation.values[2] = -1; rotation.values[8] = 1; rotation.values[10] = 0;   // world-to-view R
    temporal::Mat4 view = rotation;
    const double camera[3]{10, 2, -5};
    for (int r = 0; r < 3; ++r)
        view.values[r * 4 + 3] = -(rotation.values[r * 4 + 0] * camera[0] + rotation.values[r * 4 + 1] * camera[1] +
            rotation.values[r * 4 + 2] * camera[2]);
    temporal::Mat4 world = temporal::Mat4::identity();
    world.values[3] = 130; world.values[7] = 2; world.values[11] = -5;
    require(std::abs(camera_distance(view, world) - 120.0) < 1e-9, "camera distance from a rotated view");
    require(distance_band(camera_distance(view, world)) == 3, "120 m is the 100-150 m band");
    temporal::Mat4 projective = view;
    projective.values[14] = 1;
    require(std::isnan(camera_distance(projective, world)), "a non-rigid view has no distance");
    // Element by the engine's shader-cache root name (uber_deffer.cpp: HQ adds -hq, the tessellation pass the define list).
    require(model_element("deffer_model_bump-hq_2.vs") == ModelElement::High, "HQ bump");
    require(model_element("deffer_model_bump_d-hq_1.vs") == ModelElement::High, "HQ bump with detail");
    require(model_element("deffer_model_bump_2.vs") == ModelElement::Low, "LQ bump (beyond r__dtex_range)");
    require(model_element("deffer_model_flat_4.vs") == ModelElement::Low, "LQ flat");
    require(model_element("deffer_model_bump(tess_pn,)_3.vs") == ModelElement::Tessellated, "tessellation chain");
    require(model_element("deffer_base_bump.vs") == ModelElement::Unknown && model_element("model_env_lq_0.vs") == ModelElement::Unknown,
        "not a deferred model VS");
    // Stale palette: identical bones while the object moved more than 1 mm.
    temporal::InstanceSnapshot previous, current;
    previous.kind = current.kind = temporal::InstanceKind::Skinned;
    previous.world = current.world = temporal::Mat4::identity();
    previous.bones.assign(4, temporal::Mat4::identity());
    current.bones = previous.bones;
    require(!stale_palette(previous, current), "a character standing still is not stale");
    current.world.values[3] = 0.05;
    require(stale_palette(previous, current), "unchanged palette while walking 5 cm");
    current.world.values[3] = 0.0005;
    require(!stale_palette(previous, current), "sub-millimetre movement is noise");
    current.world.values[3] = 0.05;
    current.bones[2].values[7] = 0.01;
    require(!stale_palette(previous, current), "an animated palette is not stale");
    current.bones.pop_back();
    require(!stale_palette(previous, current), "different bone counts are never stale");
    temporal::InstanceSnapshot rigid = current;
    rigid.bones.clear();
    require(!stale_palette(previous, rigid), "a rigid snapshot has no palette");
}

const std::map<std::string, void (*)()> cases{
    {"tess-layout", tess_layout_and_packing}, {"tess-policy", tess_policy_split}, {"tess-compatibility", tess_compatibility_rules},
    {"tess-linkage", tess_linkage_rules}, {"tess-manifest", tess_manifest_rules},
    {"layouts", layouts_match_readmes}, {"core-vertex", core_vertex_packing}, {"pixel", pixel_packing},
    {"matrix", matrix_convention}, {"policy", core_policy}, {"mover", moving_character}, {"history", history_rules}, {"binder", binder_state_machine},
    {"compatibility", compatibility_rules}, {"frame", caller_frames}, {"manifest", manifest_rules}, {"loader", loader_contract},
    {"foliage-layout-readme", foliage_layout_readme}, {"foliage-layout-reflection", foliage_layout_from_reflection},
    {"foliage-packing", foliage_packing}, {"foliage-identity", foliage_identity_remap}, {"foliage-policy", foliage_policy},
    {"far-character", far_character_diagnostics}};
}

int main(int argc, char** argv)
{
    try {
        if (argc > 1) {
            const auto found = cases.find(argv[1]);
            if (found == cases.end()) { std::fprintf(stderr, "unknown case %s\n", argv[1]); return 2; }
            found->second();
        } else {
            for (const auto& [name, run] : cases) {
                try { run(); } catch (const std::exception& e) { throw std::runtime_error(name + ": " + e.what()); }
            }
        }
        std::printf("anomaly motion contract: %d checks passed\n", checks);
        return 0;
    } catch (const std::exception& error) {
        std::fprintf(stderr, "FAILED %s\n", error.what());
        return 1;
    }
}
