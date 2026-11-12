// SPDX-License-Identifier: GPL-3.0-only
// Motion MRT contracts without the game or a GPU: b13 layouts against the shader READMEs and headers, the per-draw
// history policy, the draw-time history, the RTV2 bind-preservation state machine on fake RCache memory, and the
// shader-loader hook contract (caller frame, manifest, original-bytecode proof, reflection rules, disk cache) with
// fake engine frames, a fake compiler and a fake reflector.
#include "motion_constants.h"
#include "motion_history.h"
#include "motion_policy.h"
#include "motion_profile.h"
#include "motion_target.h"
#include "shader_loader.h"
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
    require(decide_core(in).reason == HistoryReason::KindMismatch, "SKIN_0 needs a bone-composed world");
    in.kind = DrawKind::SingleBone;
    require(decide_core(in).reason == HistoryReason::UnsupportedKind, "single bone unsupported");
    in.kind = DrawKind::SoftwareSkinned; in.skin = -1;
    require(decide_core(in).reason == HistoryReason::UnsupportedKind, "software skin unsupported");
    in.kind = DrawKind::HardwareSkinned; in.skin = 1;
    auto large = skinned;
    large.bones.assign(core_max_bones + 1, sample_matrix(1));
    in.previous = &large;
    require(decide_core(in).reason == HistoryReason::TooManyBones, "palette overflow");
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
        put(frame_source_offset, &src, 8);
        put(frame_length_offset, &length, 4);
        put(frame_entry_offset, &e, 8);
        put(frame_flags_offset, &flags, 4);
        storage.clear();
        storage.reserve(defines.size() * 2);
        std::size_t i = 0;
        for (const auto& [name, value] : defines) {
            storage.push_back(name); storage.push_back(value);
            const char* pair[2]{storage[storage.size() - 2].c_str(), storage.back().c_str()};
            put(frame_defines_offset + i++ * 16, pair, 16);
        }
        const char* none[2]{terminate ? nullptr : "X", "1"};
        for (std::size_t j = i; j < frame_defines_capacity; ++j) {
            put(frame_defines_offset + j * 16, none, 16);
            if (terminate) break;
        }
    }
    const void* return_address(std::size_t call) const { return module.data() + create_shader_calls[call].rva + call_length; }
};
FrameReader direct = [](const void* a, void* o, std::size_t s) { if (!a) return false; std::memcpy(o, a, s); return true; };

void caller_frame()
{
    FakeFrame frame;
    frame.source = "#include \"common.h\"\nmain body";
    frame.defines = {{"SMAP_size", "2048"}, {"SKIN_2", "1"}, {"USE_SUNFILTER", "1"}};
    frame.build();
    for (std::size_t call = 0; call < create_shader_calls.size(); ++call) {
        ShaderRequest request;
        require(read_request(frame.module.data(), frame.return_address(call), frame.stack.data(), direct, request) == FrameStatus::Ok,
            "both create_shader call sites are accepted");
        require(std::string(reinterpret_cast<const char*>(request.source.data()), request.source.size()) == frame.source, "source");
        require(request.entry == "main" && request.flags == 8, "entry and flags");
        require(request.defines.size() == 3 && request.defines[1].name == "SKIN_2" && request.defines[0].value == "2048",
            "defines in engine order");
        require(skin_mode(request.defines) == 2, "skin mode from defines");
    }
    ShaderRequest request;
    require(read_request(frame.module.data(), frame.module.data() + 0x1234, frame.stack.data(), direct, request) ==
        FrameStatus::UnknownCaller, "any other return address is refused");
    require(read_request(frame.module.data(), static_cast<const std::byte*>(frame.return_address(0)) - 5, frame.stack.data(), direct,
        request) == FrameStatus::UnknownCaller, "the call instruction itself is not a return address");
    FakeFrame unterminated = frame;
    unterminated.build(false);
    require(read_request(unterminated.module.data(), unterminated.return_address(0), unterminated.stack.data(), direct, request) ==
        FrameStatus::BadDefines, "a define list without terminator is refused");
    const FrameReader faulting = [&](const void* a, void* o, std::size_t s) {
        if (a == frame.stack.data() + frame_entry_offset) return false;
        std::memcpy(o, a, s); return true;
    };
    require(read_request(frame.module.data(), frame.return_address(1), frame.stack.data(), faulting, request) == FrameStatus::Unreadable,
        "unreadable frame");
    std::uint32_t zero = 0;
    std::memcpy(frame.stack.data() + frame_length_offset, &zero, 4);
    require(read_request(frame.module.data(), frame.return_address(1), frame.stack.data(), direct, request) == FrameStatus::BadSource,
        "empty source");
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
    std::filesystem::remove_all(root);
}

const std::map<std::string, void (*)()> cases{
    {"layouts", layouts_match_readmes}, {"core-vertex", core_vertex_packing}, {"pixel", pixel_packing},
    {"matrix", matrix_convention}, {"policy", core_policy}, {"history", history_rules}, {"binder", binder_state_machine},
    {"compatibility", compatibility_rules}, {"frame", caller_frame}, {"manifest", manifest_rules}, {"loader", loader_contract}};
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
