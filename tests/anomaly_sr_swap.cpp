// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 1 and Stage 2 swap/restore state machine on fake CRenderTarget / CRT / CTexture / RCache memory laid out with
// the offsets of games/anomaly/src/sr_profile.h, plus fake Device.dwWidth/dwHeight, HW.pBaseZB and the SSAO helper's
// static viewport for Stage 2. Every exit path must leave the engine bytes exactly as they were.
// No game code, D3D device or executable is involved.
#include "sr_profile.h"
#include "sr_swap.h"

#include <array>
#include <climits>
#include <cmath>
#include <cstdio>
#include <limits>
#include <cstring>
#include <string_view>
#include <utility>
#include <vector>

namespace {
using namespace rsf::anomaly;
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

template<class T> void put(std::byte* base, uint32_t offset, T value) { std::memcpy(base + offset, &value, sizeof(T)); }
template<class T> T get(const std::byte* base, uint32_t offset) { T value; std::memcpy(&value, base + offset, sizeof(T)); return value; }

constexpr std::size_t g0 = slot_index(Slot::Generic0), gt = slot_index(Slot::GenericTemp), pos = slot_index(Slot::Position),
    g1 = slot_index(Slot::Generic1), g2 = slot_index(Slot::Generic2), half = slot_index(Slot::HalfDepth),
    ssao = slot_index(Slot::SsaoTemp);

struct Engine {
    alignas(16) std::array<std::byte, 0x600> target{};
    alignas(16) std::array<std::array<std::byte, crt_size>, slot_count> crt{};
    alignas(16) std::array<std::array<std::byte, 0xb0>, slot_count> texture{};
    alignas(16) std::array<std::byte, 0x900> backend{};
    // Stage 2 globals: Device.dwWidth/dwHeight, HW.pBaseZB, set_viewport's static D3D11_VIEWPORT and its guard.
    alignas(8) std::array<std::byte, 8> device_size{};
    alignas(8) std::array<std::byte, 8> base_zb{};
    alignas(8) std::array<std::byte, 8> base_rt{};
    alignas(8) std::array<std::byte, 24> viewport{};
    alignas(8) std::array<std::byte, 4> guard{};
    // Stage 3: Device.mView/mProject/mFullTransform (X-Ray Fmatrix, row vectors) and SSManager.m_uiMipLODBias.
    EngineMatrix view{}, project{}, full{};
    float sampler_bias = 0.25f;   // the user's r__tf_mipbias
    std::array<int, 64> objects{};   // addresses stand in for D3D objects
    uint32_t width = 1920, height = 1080;
    std::array<bool, slot_count> present{};
    bool half_depth_half = false;
    explicit Engine(bool optional = true, uint32_t w = 1920, uint32_t h = 1080, bool half_size = false) : width(w), height(h),
        half_depth_half(half_size) {
        static constexpr uint32_t formats[slot_count]{21, 21, 113, 21, 113, 21, 113, d3dfmt_r16f, d3dfmt_r16f};
        for (std::size_t i = 0; i < slot_count; ++i) {
            present[i] = !sr_world_targets[i].optional || optional;
            auto* c = crt[i].data();
            auto* t = texture[i].data();
            std::memset(c, 0xa0 + int(i), crt_size);           // reference count / order / padding patterns
            std::memset(t, 0xc0 + int(i), texture[i].size());
            put<void*>(c, crt_surface_offset, &objects[i * 4 + 0]);
            put<void*>(c, crt_rtv_offset, &objects[i * 4 + 1]);
            put<void*>(c, crt_dsv_offset, nullptr);
            put<void*>(c, crt_uav_offset, i == g0 || i == ssao ? &objects[i * 4 + 3] : nullptr);
            put<uint32_t>(c, crt_format_offset, formats[i]);
            put<void*>(c, crt_texture_offset, t);
            put<void*>(t, texture_surface_offset, &objects[i * 4 + 0]);
            put<void*>(t, texture_desc_cache_offset, &objects[i * 4 + 0]);
            put<void*>(t, texture_srv_offset, &objects[i * 4 + 2]);
            put<void*>(target.data(), sr_world_targets[i].offset, present[i] ? c : nullptr);
        }
        size_targets();
        put<uint32_t>(target.data(), target_width_offset, w);
        put<uint32_t>(target.data(), target_height_offset, h);
        std::memset(backend.data(), 0x5a, backend.size());
        put<uint32_t>(device_size.data(), 0, w);
        put<uint32_t>(device_size.data(), 4, h);
        put<void*>(base_zb.data(), 0, &objects[60]);
        put<void*>(base_rt.data(), 0, &objects[63]);
        put<float>(viewport.data(), 0x14, 1.0f);   // MaxDepth: the .data initialiser
        project = perspective(1.2f, float(w) / float(h));
        // A camera rotated about Y and moved (Fmatrix::build_camera_dir-like rows, translation in the last row).
        view = {0.8f, 0.0f, -0.6f, 0.0f, 0.0f, 1.0f, 0.0f, 0.0f, 0.6f, 0.0f, 0.8f, 0.0f, -3.0f, -1.5f, 7.25f, 1.0f};
        full = engine_mul(project, view);
    }
    // Fmatrix::build_projection(fov, aspect, near 0.2, far 1000): LH, D3D depth, _34 = 1.
    static EngineMatrix perspective(float fov, float aspect, float near_plane = 0.2f, float far_plane = 1000.0f) {
        const float h = 1.0f / std::tan(fov * 0.5f), w = h / aspect, q = far_plane / (far_plane - near_plane);
        return {w, 0, 0, 0, 0, h, 0, 0, 0, 0, q, 1, 0, 0, -q * near_plane, 0};
    }
    void size_targets() {
        for (std::size_t i = 0; i < slot_count; ++i) {
            const bool small = i == half && half_depth_half;
            put<uint32_t>(crt[i].data(), crt_width_offset, small ? width / 2 : width);
            put<uint32_t>(crt[i].data(), crt_height_offset, small ? height / 2 : height);
        }
    }
    void resize(uint32_t w, uint32_t h) {
        width = w; height = h;
        size_targets();
        put<uint32_t>(device_size.data(), 0, w);
        put<uint32_t>(device_size.data(), 4, h);
        put<uint32_t>(target.data(), target_width_offset, w);
        put<uint32_t>(target.data(), target_height_offset, h);
    }
    // An initialised magic static holding the stock first-call value.
    void init_viewport(float w, float h) {
        put<int32_t>(guard.data(), 0, INT_MIN + 7);
        put<float>(viewport.data(), ssao_viewport_width_offset, w);
        put<float>(viewport.data(), ssao_viewport_height_offset, h);
    }
    FrameFacts facts() { return {target.data(), backend.data(), 0x1234, 0, width, height, false, false}; }
    FrameFacts facts2(float scale = 0.67f) {
        auto f = facts();
        ScalePlan plan{};
        make_plan(width, height, scale, plan);
        f.stage = 2;
        f.render_width = plan.render_width;
        f.render_height = plan.render_height;
        f.device_size = device_size.data();
        f.base_zb = base_zb.data();
        f.base_rt = base_rt.data();
        f.ssao_viewport = viewport.data();
        f.ssao_viewport_guard = guard.data();
        return f;
    }
    FrameFacts facts3(float scale = 0.67f, float jx = 0.25f, float jy = -1.0f / 6.0f, bool jitter = true, bool mip = true) {
        auto f = facts2(scale);
        f.jitter = jitter;
        f.jitter_x = jx;
        f.jitter_y = jy;
        f.device_view = reinterpret_cast<std::byte*>(view.data());
        f.device_project = reinterpret_cast<std::byte*>(project.data());
        f.device_full_transform = reinterpret_cast<std::byte*>(full.data());
        f.mip_bias = mip;
        f.mip_bias_delta = window_mip_bias_delta(f.render_width, f.device_width, default_mip_bias_offset);
        f.sampler_bias = reinterpret_cast<std::byte*>(&sampler_bias);
        return f;
    }
    std::vector<std::byte> snapshot() const {
        std::vector<std::byte> bytes;
        for (const auto& c : crt) bytes.insert(bytes.end(), c.begin(), c.end());
        for (const auto& t : texture) bytes.insert(bytes.end(), t.begin(), t.end());
        bytes.insert(bytes.end(), target.begin(), target.end());
        for (const auto* g : {&device_size, &base_zb, &base_rt}) bytes.insert(bytes.end(), g->begin(), g->end());
        bytes.insert(bytes.end(), viewport.begin(), viewport.end());
        bytes.insert(bytes.end(), guard.begin(), guard.end());
        for (const auto* m : {&view, &project, &full}) {
            const auto* raw = reinterpret_cast<const std::byte*>(m->data());
            bytes.insert(bytes.end(), raw, raw + sizeof(EngineMatrix));
        }
        const auto* bias = reinterpret_cast<const std::byte*>(&sampler_bias);
        bytes.insert(bytes.end(), bias, bias + sizeof(float));
        return bytes;
    }
    bool texture_cache_cleared() const {
        if (get<void*>(backend.data(), backend_texture_list_offset)) return false;
        for (uint32_t i = 0; i < backend_textures_ps_count; ++i) if (get<void*>(backend.data(), backend_textures_ps_offset + 8 * i)) return false;
        for (uint32_t i = 0; i < backend_textures_vs_count; ++i) if (get<void*>(backend.data(), backend_textures_vs_offset + 8 * i)) return false;
        for (const auto offset : {backend_textures_gs_offset, backend_textures_hs_offset, backend_textures_ds_offset, backend_textures_cs_offset})
            for (uint32_t i = 0; i < backend_textures_stage_count; ++i) if (get<void*>(backend.data(), offset + 8 * i)) return false;
        // Fields around the cleared ranges are untouched.
        return get<uint8_t>(backend.data(), backend_texture_list_offset - 1) == 0x5a &&
            get<uint8_t>(backend.data(), backend_textures_cs_offset + 8 * backend_textures_stage_count) == 0x5a;
    }
    bool ctable_cleared() const {
        return !get<void*>(backend.data(), backend_ctable_offset) && get<uint8_t>(backend.data(), backend_ctable_offset - 1) == 0x5a &&
            get<uint8_t>(backend.data(), backend_ctable_offset + 8) == 0x5a;
    }
    void dirty_backend() { std::memset(backend.data(), 0x5a, backend.size()); }
};

struct FakeHost final : SwapHost {
    std::array<int, 64> twin_objects{};
    int write_count = 0, fail_write_at = -1;
    bool fail_make = false, fail_round_trip = false, wrong_format = false, depth_refused = false, refused = false;
    int makes = 0, drops = 0, round_trips = 0, upscales = 0;
    TargetFields last_twin{}, last_engine{};
    TwinRequest last_request{};
    struct Upscale { TargetFields g0_twin, pos_twin, g1_twin, g0, pos, g1; void* depth_srv; void* dsv; uint32_t rw, rh, ow, oh; } last_upscale{};
    std::vector<std::pair<uint32_t, uint32_t>> viewports;
    std::vector<std::byte> engine_at_round_trip;
    Engine* engine{};
    bool read(const void* address, void* output, std::size_t size) noexcept override {
        if (!address) return false;
        std::memcpy(output, address, size);
        return true;
    }
    bool write(void* address, const void* input, std::size_t size) noexcept override {
        if (!address || write_count++ == fail_write_at) return false;
        std::memcpy(address, input, size);
        return true;
    }
    MakeResult make_twins(const TwinRequest& r, TwinSet& out) noexcept override {
        ++makes;
        last_request = r;
        if (fail_make) return MakeResult::Failed;
        if (depth_refused) return MakeResult::Depth;
        if (refused) return MakeResult::Refused;
        for (std::size_t i = 0; i < slot_count; ++i) {
            if (!r.spec[i].present) continue;
            auto& f = out.targets[i];
            f.surface = f.texture_surface = f.desc_cache = &twin_objects[i * 4 + 0];
            f.rtv = &twin_objects[i * 4 + 1];
            f.srv = &twin_objects[i * 4 + 2];
            f.uav = r.engine[i].fields.uav ? &twin_objects[i * 4 + 3] : nullptr;
            f.width = r.spec[i].width;
            f.height = r.spec[i].height;
            f.format = wrong_format ? d3dfmt_a8r8g8b8 : r.spec[i].promote ? d3dfmt_a16b16g16r16f : r.engine[i].fields.format;
            if (wrong_format && !r.spec[i].promote) f.format = 1;
            f.desc.fill(std::byte{0x77});
        }
        if (r.stage == 2) {
            out.depth_dsv = &twin_objects[60];
            out.depth_srv = &twin_objects[61];
            out.depth_width = r.render_width;
            out.depth_height = r.render_height;
            out.base_rt = &twin_objects[62];
        }
        return MakeResult::Created;
    }
    void drop_twins() noexcept override { ++drops; }
    bool round_trip(const TargetFields& twin, const TargetFields& original) noexcept override {
        ++round_trips;
        last_twin = twin;
        last_engine = original;
        if (engine) engine_at_round_trip = engine->snapshot();
        return !fail_round_trip;
    }
    bool upscale(const UpscaleInputs& in) noexcept override {
        ++upscales;
        last_upscale = {*in.generic0_twin, *in.position_twin, *in.generic1_twin, *in.generic0, *in.position, *in.generic1,
            in.depth_srv, in.engine_dsv, in.render_width, in.render_height, in.output_width, in.output_height};
        if (engine) engine_at_round_trip = engine->snapshot();
        return !fail_round_trip;
    }
    void set_viewport(uint32_t w, uint32_t h) noexcept override { viewports.emplace_back(w, h); }
    // Stage 3: R_xforms::set_P records the matrix it was given; SetMipLODBias stores the value as the engine does.
    int upload_calls = 0, fail_upload_at = -1, bias_calls = 0, fail_bias_at = -1;
    std::vector<EngineMatrix> uploaded;
    std::vector<float> biases;
    bool upload_projection(const void* matrix) noexcept override {
        if (upload_calls++ == fail_upload_at) return false;
        EngineMatrix m{};
        std::memcpy(m.data(), matrix, sizeof(m));
        uploaded.push_back(m);
        return true;
    }
    bool set_mip_bias(float value) noexcept override {
        if (bias_calls++ == fail_bias_at) return false;
        biases.push_back(value);
        if (engine) engine->sampler_bias = value;
        return true;
    }
};

bool slot_points_at_twin(const Engine& engine, const SwapController& controller, std::size_t i) {
    const auto& twin = controller.twins()[i];
    const auto* c = engine.crt[i].data();
    const auto* t = engine.texture[i].data();
    if (get<void*>(c, crt_surface_offset) != twin.surface || get<void*>(c, crt_rtv_offset) != twin.rtv ||
        get<void*>(c, crt_uav_offset) != twin.uav || get<uint32_t>(c, crt_format_offset) != twin.format ||
        get<uint32_t>(c, crt_width_offset) != twin.width || get<uint32_t>(c, crt_height_offset) != twin.height ||
        get<void*>(t, texture_surface_offset) != twin.surface || get<void*>(t, texture_desc_cache_offset) != twin.surface ||
        get<void*>(t, texture_srv_offset) != twin.srv || get<uint8_t>(t, texture_desc_offset) != 0x77)
        return false;
    // The CRT reference count, CTexture pointer and order are never exchanged.
    return get<void*>(c, crt_texture_offset) == t && get<uint8_t>(c, crt_reference_offset) == 0xa0 + i &&
        get<uint8_t>(c, crt_order_offset) == 0xa0 + i && get<uint8_t>(t, texture_reference_offset) == 0xc0 + i;
}
bool engine_points_at_twins(const Engine& engine, const SwapController& controller, std::size_t count = stage1_slot_count) {
    for (std::size_t i = 0; i < count; ++i) {
        if (!engine.present[i]) continue;
        if (!slot_points_at_twin(engine, controller, i)) return false;
        if (i < stage1_slot_count && controller.twins()[i].format != d3dfmt_a16b16g16r16f) return false;
    }
    return true;
}
std::size_t skips(const SwapController& c, Skip s) { return c.counters().skipped[static_cast<std::size_t>(s)]; }
std::size_t restores(const SwapController& c, Restore r) { return c.counters().restores[static_cast<std::size_t>(r)]; }

struct Case {
    Engine engine;
    FakeHost host;
    SwapController controller{host};
    std::vector<std::byte> original;
    explicit Case(bool optional = true, uint32_t w = 1920, uint32_t h = 1080, bool half_size = false) : engine(optional, w, h, half_size) {
        host.engine = &engine; controller.set_swap_enabled(true); original = engine.snapshot();
    }
    void rebase() { original = engine.snapshot(); }
    bool stock() const { return engine.snapshot() == original; }
};

// ---- Stage 1 -----------------------------------------------------------------------------------------------------
void normal_frame() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    CHECK(c.controller.window_open() && engine_points_at_twins(c.engine, c.controller) && c.engine.texture_cache_cleared());
    CHECK(get<uint8_t>(c.engine.backend.data(), backend_ctable_offset) == 0x5a);   // Stage 1 keeps the constant cache
    // Stage 1 never touches the other world targets or the globals.
    for (std::size_t i = stage1_slot_count; i < slot_count; ++i)
        CHECK(get<void*>(c.engine.crt[i].data(), crt_surface_offset) == &c.engine.objects[i * 4]);
    CHECK(get<uint32_t>(c.engine.device_size.data(), 0) == 1920 && get<void*>(c.engine.base_zb.data(), 0) == &c.engine.objects[60]);
    c.controller.prepared();
    CHECK(c.host.viewports.empty());
    c.controller.rm_near();
    c.engine.dirty_backend();
    CHECK(c.controller.seam());                     // phase_sunshafts: the seam
    CHECK(c.stock() && !c.controller.window_open() && c.engine.texture_cache_cleared());
    CHECK(c.host.round_trips == 1 && c.host.upscales == 0 && c.host.engine_at_round_trip == c.original);   // the pass runs on restored fields
    CHECK(c.host.last_twin == c.controller.twins()[0] && c.host.last_engine.surface == &c.engine.objects[0] &&
        c.host.last_engine.rtv == &c.engine.objects[1]);
    CHECK(!c.controller.seam());                    // phase_blur in the same frame passes through
    c.controller.combine_end();
    c.controller.frame_end();
    c.controller.rm_near();                         // outside the window: not counted
    CHECK(c.stock() && !c.controller.refused() && c.host.viewports.empty());
    const auto& n = c.controller.counters();
    CHECK(n.prepares == 1 && n.eligible == 1 && n.swapped == 1 && n.seams == 1 && n.seams_without_window == 1 &&
        n.round_trips == 1 && n.rm_near_in_window == 1 && n.twin_generations == 1 && restores(c.controller, Restore::Seam) == 1);
    // Next frames reuse the twins.
    for (int frame = 0; frame < 3; ++frame) {
        CHECK(c.controller.prepare(c.engine.facts()));
        CHECK(c.controller.seam());
        c.controller.combine_end();
        CHECK(c.stock());
    }
    CHECK(c.host.makes == 1 && c.controller.counters().swapped == 4);
    CHECK(c.host.last_request.stage == 1 && c.host.last_request.spec[0].promote && !c.host.last_request.spec[pos].present);
    c.controller.shutdown();
    CHECK(c.stock() && c.host.drops == 1);
}

void blur_only_frame() {
    // ps_sunshafts_mode 0/1: phase_blur is the first seam call.
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    CHECK(c.controller.seam());
    c.controller.combine_end();
    CHECK(c.stock() && c.controller.counters().seams_without_window == 0);
}

void combine_without_seam() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    c.controller.combine_end();
    CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::CombineEnd) == 1 && c.host.round_trips == 0);
    CHECK(!c.controller.prepare(c.engine.facts()) && skips(c.controller, Skip::Off) == 1 && c.stock());
}

void stale_prepare() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    CHECK(c.controller.prepare(c.engine.facts()));   // the previous window is restored first, then reopened
    CHECK(restores(c.controller, Restore::StalePrepare) == 1 && engine_points_at_twins(c.engine, c.controller));
    CHECK(c.controller.seam() && c.stock());
    // The hook's own close_stale before it gathers facts.
    CHECK(c.controller.prepare(c.engine.facts()));
    c.controller.close_stale();
    CHECK(c.stock() && restores(c.controller, Restore::StalePrepare) == 2 && !c.controller.window_open());
}

void frame_end_open() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    c.controller.frame_end();
    CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::FrameEnd) == 1);
}

void shutdown_open() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    c.controller.shutdown();
    CHECK(c.stock() && !c.controller.window_open() && c.host.drops == 1 && restores(c.controller, Restore::Shutdown) == 1);
    c.controller.shutdown();   // idempotent
    CHECK(c.stock() && c.host.drops == 1);
}

void ineligible_frames() {
    Case c;
    auto facts = c.engine.facts();
    facts.scope = true;
    CHECK(!c.controller.prepare(facts) && c.stock() && skips(c.controller, Skip::Scope) == 1);
    facts.scope = false;
    facts.msaa = true;
    CHECK(!c.controller.prepare(facts) && c.stock() && skips(c.controller, Skip::Msaa) == 1);
    // Menu / loading frames never reach H1; their seam and combine calls pass through.
    CHECK(!c.controller.seam());
    c.controller.combine_end();
    c.controller.frame_end();
    CHECK(c.stock() && !c.controller.refused() && c.host.makes == 0);
    // Stock capture runs: swap disabled.
    c.controller.set_swap_enabled(false);
    CHECK(!c.controller.prepare(c.engine.facts()) && skips(c.controller, Skip::Off) == 1 && c.stock());
    // And an SVP frame in between real frames does not disturb the next one.
    c.controller.set_swap_enabled(true);
    CHECK(c.controller.prepare(c.engine.facts()) && c.controller.seam() && c.stock());
}

void layout_mismatch() {
    const auto refused_untouched = [](auto mutate) {
        Case c;
        mutate(c.engine);
        c.rebase();
        const bool opened = c.controller.prepare(c.engine.facts());
        return !opened && c.stock() && c.controller.refused() && skips(c.controller, Skip::Layout) == 1 && c.host.makes == 0;
    };
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[0].data(), crt_format_offset, 28); }));
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[1].data(), crt_width_offset, 960); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.texture[0].data(), texture_srv_offset, nullptr); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.texture[1].data(), texture_surface_offset, &e.objects[15]); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.crt[0].data(), crt_dsv_offset, &e.objects[15]); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), 0x3b8, nullptr); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), 0x3b8, e.crt[0].data()); }));
    Case c;
    auto facts = c.engine.facts();
    facts.render_target = nullptr;   // `this` was not CRender::Target
    CHECK(!c.controller.prepare(facts) && c.stock() && c.controller.refused());
}

void twin_failures() {
    {
        // An allocation failure is not a refusal: this shape stays stock, a new shape is tried again.
        Case c;
        c.host.fail_make = true;
        CHECK(!c.controller.prepare(c.engine.facts()) && c.stock() && !c.controller.refused() && skips(c.controller, Skip::Twins) == 1);
        CHECK(!c.controller.prepare(c.engine.facts()) && c.host.makes == 1 && skips(c.controller, Skip::Twins) == 2);
        c.host.fail_make = false;
        auto facts = c.engine.facts();
        facts.generation = 1;   // a reset
        CHECK(c.controller.prepare(facts) && c.host.makes == 2 && c.controller.seam() && c.stock());
        CHECK(c.controller.counters().allocation_failures == 1);
    }
    {
        Case c;
        c.host.wrong_format = true;   // a twin that is not FP16 is refused and dropped
        CHECK(!c.controller.prepare(c.engine.facts()) && c.stock() && c.controller.refused() && c.host.drops == 1);
    }
    {
        Case c;
        c.host.refused = true;        // the host found an engine target it cannot twin
        CHECK(!c.controller.prepare(c.engine.facts()) && c.stock() && c.controller.refused() && skips(c.controller, Skip::Twins) == 1);
    }
}

void write_fault_rolls_back() {
    // Fail at every write position of the exchange (11 field writes per slot).
    for (int at = 0; at < 22; ++at) {
        Case c;
        c.host.fail_write_at = at;
        CHECK(!c.controller.prepare(c.engine.facts()));
        CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::Rollback) == 1 && !c.controller.window_open());
    }
}

void foreign_change() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    int replacement{};
    put<void*>(c.engine.texture[0].data(), texture_srv_offset, &replacement);   // as surface_set would
    CHECK(c.controller.seam());
    CHECK(c.stock() && c.controller.refused() && c.controller.counters().foreign_changes == 1);
}

void round_trip_failure() {
    Case c;
    c.host.fail_round_trip = true;
    CHECK(c.controller.prepare(c.engine.facts()));
    CHECK(c.controller.seam());
    CHECK(c.stock() && c.controller.refused() && c.controller.counters().round_trip_failures == 1);
}

void reset_and_resize() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()) && c.controller.seam());
    auto facts = c.engine.facts();
    facts.generation = 1;   // vid_restart: a reset at the same size still drops and recreates
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock());
    CHECK(c.host.makes == 2 && c.host.drops == 1 && c.controller.twin_generation() == 2);
    c.engine.resize(1280, 720);
    c.rebase();
    facts = c.engine.facts();
    facts.generation = 2;
    CHECK(c.controller.prepare(facts) && engine_points_at_twins(c.engine, c.controller));
    CHECK(c.controller.twins()[0].width == 1280 && c.controller.twins()[1].height == 720);
    CHECK(c.controller.seam() && c.stock() && c.host.makes == 3 && c.host.drops == 2);
    // A recreated CRT (new surface address) at the same generation also rebuilds.
    put<void*>(c.engine.crt[0].data(), crt_surface_offset, &c.engine.objects[59]);
    put<void*>(c.engine.texture[0].data(), texture_surface_offset, &c.engine.objects[59]);
    put<void*>(c.engine.texture[0].data(), texture_desc_cache_offset, nullptr);   // desc not yet cached
    c.rebase();
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.makes == 4);
    // The device changes (a new device after a full restart).
    facts.device = 0x5678;
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.makes == 5);
}

void names() {
    CHECK(std::string_view{skip_name(Skip::Scope)} == "scope" && std::string_view{skip_name(Skip::Count)} == "unknown");
    CHECK(std::string_view{skip_name(Skip::Depth)} == "depth");
    CHECK(std::string_view{restore_name(Restore::FrameEnd)} == "frame_end");
}

// ---- Stage 2 -----------------------------------------------------------------------------------------------------
bool globals_in_window(const Engine& e, const SwapController& c) {
    return get<uint32_t>(e.device_size.data(), 0) == c.render_width() && get<uint32_t>(e.device_size.data(), 4) == c.render_height() &&
        get<void*>(e.base_zb.data(), 0) == c.twin_set().depth_dsv && get<void*>(e.base_rt.data(), 0) == c.twin_set().base_rt &&
        c.twin_set().base_rt != nullptr;
}

void stage2_normal() {
    Case c;
    c.engine.init_viewport(960.0f, 540.0f);
    c.rebase();
    const auto facts = c.engine.facts2();
    CHECK(facts.render_width == 1286 && facts.render_height == 724);
    CHECK(c.controller.prepare(facts));
    CHECK(c.controller.stage() == 2 && c.controller.render_width() == 1286 && c.controller.output_width() == 1920);
    CHECK(engine_points_at_twins(c.engine, c.controller, slot_count));
    for (std::size_t i = 0; i < slot_count; ++i) {
        CHECK(c.controller.twins()[i].width == 1286 && c.controller.twins()[i].height == 724);
        CHECK(c.host.last_request.spec[i].present && c.host.last_request.spec[i].promote == (i < stage1_slot_count));
    }
    CHECK(c.controller.twins()[pos].format == 113 && c.controller.twins()[half].format == d3dfmt_r16f && c.controller.twins()[ssao].uav);
    CHECK(globals_in_window(c.engine, c.controller) && c.controller.twin_set().depth_width == 1286);
    CHECK(get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 643.0f &&
        get<float>(c.engine.viewport.data(), ssao_viewport_height_offset) == 362.0f);
    CHECK(c.engine.texture_cache_cleared() && c.engine.ctable_cleared());
    CHECK(c.host.last_request.engine_dsv == &c.engine.objects[60] && c.host.last_request.output_width == 1920);
    // H1 epilogue: render-size viewport. The engine's u_setrt then sets CRenderTarget::dwWidth to the twin size.
    c.controller.prepared();
    CHECK(c.host.viewports.size() == 1 && c.host.viewports[0] == std::make_pair(1286u, 724u));
    put<uint32_t>(c.engine.target.data(), target_width_offset, 1286);
    put<uint32_t>(c.engine.target.data(), target_height_offset, 724);
    c.controller.rm_near();
    c.engine.dirty_backend();
    CHECK(c.controller.seam());
    // Everything is back, including Device size, HW.pBaseZB, the SSAO static and CRenderTarget::dwWidth/dwHeight.
    CHECK(c.stock() && c.engine.texture_cache_cleared() && c.engine.ctable_cleared());
    CHECK(c.host.upscales == 1 && c.host.round_trips == 0 && c.host.engine_at_round_trip == c.original);
    const auto& u = c.host.last_upscale;
    CHECK(u.g0_twin == c.controller.twins()[g0] && u.pos_twin == c.controller.twins()[pos] && u.g1_twin == c.controller.twins()[g1]);
    CHECK(u.g0.surface == &c.engine.objects[g0 * 4] && u.pos.rtv == &c.engine.objects[pos * 4 + 1] && u.g1.rtv == &c.engine.objects[g1 * 4 + 1]);
    CHECK(u.depth_srv == c.controller.twin_set().depth_srv && u.dsv == &c.engine.objects[60]);
    CHECK(u.rw == 1286 && u.rh == 724 && u.ow == 1920 && u.oh == 1080);
    CHECK(c.host.viewports.size() == 2 && c.host.viewports[1] == std::make_pair(1920u, 1080u));
    CHECK(!c.controller.seam());   // phase_blur passes through
    c.controller.combine_end();
    c.controller.frame_end();
    CHECK(c.stock() && !c.controller.refused());
    for (int frame = 0; frame < 3; ++frame) {
        CHECK(c.controller.prepare(c.engine.facts2()));
        c.controller.prepared();
        CHECK(c.controller.seam());
        c.controller.combine_end();
        CHECK(c.stock());
    }
    const auto& n = c.controller.counters();
    CHECK(c.host.makes == 1 && n.swapped == 4 && n.round_trips == 4 && n.ssao_viewport_swaps == 4 && n.viewports == 8);
    CHECK(n.rm_near_in_window == 1 && restores(c.controller, Restore::Seam) == 4);
    c.controller.shutdown();
    CHECK(c.stock() && c.host.drops == 1);
}

void stage2_optional_and_odd() {
    {
        // Options off: generic2, half_depth and ssao_temp are null and stay untouched.
        Case c(false);
        CHECK(c.controller.prepare(c.engine.facts2()));
        CHECK(engine_points_at_twins(c.engine, c.controller, slot_count));
        for (const auto i : {g2, half, ssao}) {
            CHECK(!c.host.last_request.spec[i].present && c.controller.twins()[i] == TargetFields{});
            CHECK(!get<void*>(c.engine.target.data(), sr_world_targets[i].offset));
        }
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // Odd output, half-size SSAO data: the half target keeps the engine's integer halving of the render size.
        Case c(true, 1919, 1079, true);
        const auto facts = c.engine.facts2();
        CHECK(facts.render_width == 1286 && facts.render_height == 723);
        CHECK(c.controller.prepare(facts));
        CHECK(c.controller.twins()[half].width == 643 && c.controller.twins()[half].height == 361);
        CHECK(c.controller.twins()[g0].width == 1286 && c.controller.twins()[g0].height == 723);
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // Minimum scale and SRScale=1.0: render size = output size, every twin at output size.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2(0.5f)) && c.controller.render_width() == 960 && c.controller.render_height() == 540);
        CHECK(c.controller.seam() && c.stock());
        CHECK(c.controller.prepare(c.engine.facts2(1.0f)) && c.controller.render_width() == 1920 && c.controller.twins()[pos].width == 1920);
        CHECK(globals_in_window(c.engine, c.controller) && get<uint32_t>(c.engine.device_size.data(), 0) == 1920);
        CHECK(c.controller.seam() && c.stock() && c.host.makes == 2);
    }
}

void stage2_layout() {
    const auto refused_untouched = [](auto mutate, Skip reason = Skip::Layout) {
        Case c;
        c.engine.init_viewport(960.0f, 540.0f);
        mutate(c.engine);
        c.rebase();
        auto facts = c.engine.facts2();
        const bool opened = c.controller.prepare(facts);
        return !opened && c.stock() && c.controller.refused() && skips(c.controller, reason) == 1 && c.host.makes == 0;
    };
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[pos].data(), crt_format_offset, 21); }));
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[half].data(), crt_format_offset, 21); }));
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[half].data(), crt_width_offset, 480); }));   // output/4
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[g1].data(), crt_width_offset, 960); }));     // half not allowed
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.crt[half].data(), crt_width_offset, 960); }));   // halved on one axis
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), sr_world_targets[pos].offset, nullptr); }));   // required
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), sr_world_targets[g2].offset, e.crt[g1].data()); }));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), 0x390, e.crt[g2].data()); }));   // accum_temp present
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.target.data(), 0x358, e.crt[g2].data()); }));   // msaadepth present
    CHECK(refused_untouched([](Engine& e) { put<uint32_t>(e.device_size.data(), 0, 1600); }));           // Device size != facts
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.base_zb.data(), 0, nullptr); }, Skip::Depth));
    CHECK(refused_untouched([](Engine& e) { put<void*>(e.base_rt.data(), 0, nullptr); }));
    {
        Case c;
        c.host.depth_refused = true;   // a DSV that is not D24S8 at output size
        CHECK(!c.controller.prepare(c.engine.facts2()) && c.stock() && c.controller.refused() && skips(c.controller, Skip::Depth) == 1);
    }
    {
        Case c;
        auto facts = c.engine.facts2();
        facts.base_zb = nullptr;
        CHECK(!c.controller.prepare(facts) && c.stock() && c.controller.refused());
    }
    {
        Case c;
        auto facts = c.engine.facts2();
        facts.render_width = 2000;   // larger than the output
        CHECK(!c.controller.prepare(facts) && c.stock() && c.controller.refused());
    }
    {
        Case c;
        c.host.wrong_format = true;   // a kept-format twin with another format
        CHECK(!c.controller.prepare(c.engine.facts2()) && c.stock() && c.controller.refused() && c.host.drops == 1);
    }
}

void stage2_exits() {
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        c.controller.prepared();
        put<uint32_t>(c.engine.target.data(), target_width_offset, 1286);   // u_setrt inside the window
        c.controller.combine_end();
        CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::CombineEnd) == 1 && c.host.upscales == 0);
        CHECK(c.host.viewports.back() == std::make_pair(1920u, 1080u));
    }
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        c.controller.frame_end();
        CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::FrameEnd) == 1);
        CHECK(c.host.viewports.back() == std::make_pair(1920u, 1080u));
    }
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        c.controller.close_stale();
        CHECK(c.stock() && !c.controller.refused() && restores(c.controller, Restore::StalePrepare) == 1);
        CHECK(c.host.viewports.back() == std::make_pair(1920u, 1080u));
        CHECK(c.controller.prepare(c.engine.facts2()) && c.controller.seam() && c.stock());
    }
    {
        // Shutdown (quiesce) restores engine memory but makes no D3D call.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        c.controller.shutdown();
        CHECK(c.stock() && c.host.viewports.empty() && c.host.drops == 1 && restores(c.controller, Restore::Shutdown) == 1);
    }
    {
        Case c;
        c.host.fail_round_trip = true;
        CHECK(c.controller.prepare(c.engine.facts2()) && c.controller.seam());
        CHECK(c.stock() && c.controller.refused() && c.controller.counters().round_trip_failures == 1);
    }
    {
        // Ineligible frames leave every global alone.
        Case c;
        auto facts = c.engine.facts2();
        facts.scope = true;
        CHECK(!c.controller.prepare(facts) && skips(c.controller, Skip::Scope) == 1);
        facts.scope = false;
        facts.msaa = true;
        CHECK(!c.controller.prepare(facts) && skips(c.controller, Skip::Msaa) == 1);
        c.controller.prepared();
        CHECK(!c.controller.seam());
        c.controller.combine_end();
        CHECK(c.stock() && c.host.makes == 0 && c.host.viewports.empty() && !c.controller.refused());
        c.controller.set_swap_enabled(false);
        CHECK(!c.controller.prepare(c.engine.facts2()) && c.stock() && skips(c.controller, Skip::Off) == 1);
    }
}

void stage2_foreign() {
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        put<uint32_t>(c.engine.device_size.data(), 0, 800);   // something wrote Device.dwWidth inside the window
        CHECK(c.controller.seam());
        CHECK(c.stock() && c.controller.refused() && c.controller.counters().foreign_changes == 1);
    }
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        put<void*>(c.engine.base_zb.data(), 0, &c.engine.objects[50]);   // HW.pBaseZB replaced (UpdateViews)
        CHECK(c.controller.seam());
        CHECK(c.stock() && c.controller.refused() && c.controller.counters().foreign_changes == 1);
    }
    {
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        put<void*>(c.engine.base_rt.data(), 0, &c.engine.objects[51]);   // HW.pBaseRT replaced
        CHECK(c.controller.seam());
        CHECK(c.stock() && c.controller.refused() && c.controller.counters().foreign_changes == 1);
    }
}

void stage2_ssao_viewport() {
    {
        // Initialised before the window with a stale stock value (an earlier resolution): saved and restored exactly.
        Case c;
        c.engine.init_viewport(640.0f, 360.0f);
        c.rebase();
        CHECK(c.controller.prepare(c.engine.facts2()));
        CHECK(get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 643.0f);
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // Never initialised: untouched in the window. The first SSAO call inside the window stores render/2 (what the
        // engine's init path does there); the restore stores output/2, what stock would have stored.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts2()));
        CHECK(get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 0.0f && c.controller.counters().ssao_viewport_swaps == 0);
        put<int32_t>(c.engine.guard.data(), 0, INT_MIN + 1);
        put<float>(c.engine.viewport.data(), ssao_viewport_width_offset, 643.0f);
        put<float>(c.engine.viewport.data(), ssao_viewport_height_offset, 362.0f);
        CHECK(c.controller.seam());
        CHECK(get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 960.0f &&
            get<float>(c.engine.viewport.data(), ssao_viewport_height_offset) == 540.0f && !c.controller.refused());
        // From now on it is swapped like any initialised static.
        c.rebase();
        CHECK(c.controller.prepare(c.engine.facts2()) && get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 643.0f);
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // Initialisation in progress on another thread (-1): left alone.
        Case c;
        put<int32_t>(c.engine.guard.data(), 0, -1);
        c.rebase();
        CHECK(c.controller.prepare(c.engine.facts2()));
        CHECK(get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 0.0f);
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // No static address (not mapped): nothing is touched.
        Case c;
        c.engine.init_viewport(960.0f, 540.0f);
        c.rebase();
        auto facts = c.engine.facts2();
        facts.ssao_viewport = facts.ssao_viewport_guard = nullptr;
        CHECK(c.controller.prepare(facts) && get<float>(c.engine.viewport.data(), ssao_viewport_width_offset) == 960.0f);
        CHECK(c.controller.seam() && c.stock());
    }
}

void stage2_write_faults() {
    // Count the writes of a clean exchange, then fail at each one: every engine byte, including the globals and the
    // SSAO static, must come back and further swaps are refused.
    Case probe;
    probe.engine.init_viewport(960.0f, 540.0f);
    probe.rebase();
    probe.host.write_count = 0;
    probe.host.fail_write_at = -1;
    CHECK(probe.controller.prepare(probe.engine.facts2()));
    // 9 slots x 11 field writes, Device width/height, HW.pBaseZB, HW.pBaseRT, the viewport pair, then the caches.
    constexpr int exchange_writes = int(slot_count) * 11 + 2 + 1 + 1 + 1;
    constexpr int cache_writes = 1 + int(backend_textures_ps_count + backend_textures_vs_count + 4 * backend_textures_stage_count) + 1;
    CHECK(probe.host.write_count == exchange_writes + cache_writes);
    for (int at = 0; at < exchange_writes; ++at) {
        Case c;
        c.engine.init_viewport(960.0f, 540.0f);
        c.rebase();
        c.host.fail_write_at = at;
        CHECK(!c.controller.prepare(c.engine.facts2()));
        CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::Rollback) == 1 && !c.controller.window_open());
        CHECK(c.host.viewports.empty());
    }
    // A failed cache invalidation keeps the window (the fields are consistent) but refuses the next frames.
    Case c;
    c.host.fail_write_at = exchange_writes - 1;   // guard unset: no viewport pair, so this is the first invalidation write
    CHECK(c.controller.prepare(c.engine.facts2()) && c.controller.refused() && c.controller.counters().write_faults == 1);
    CHECK(c.controller.seam() && c.stock());
}

void stage2_lifecycle() {
    Case c;
    c.engine.init_viewport(960.0f, 540.0f);
    c.rebase();
    CHECK(c.controller.prepare(c.engine.facts2()) && c.controller.seam() && c.stock());
    // Scale change: drop and rebuild at the new render size.
    CHECK(c.controller.prepare(c.engine.facts2(0.5f)) && c.controller.twins()[g0].width == 960 && globals_in_window(c.engine, c.controller));
    CHECK(c.controller.seam() && c.stock() && c.host.makes == 2 && c.host.drops == 1 && c.controller.twin_generation() == 2);
    // Same scale again: reused.
    CHECK(c.controller.prepare(c.engine.facts2(0.5f)) && c.controller.seam() && c.host.makes == 2);
    // vid_restart at the same size: a new generation and a new HW.pBaseZB view.
    put<void*>(c.engine.base_zb.data(), 0, &c.engine.objects[61]);
    c.rebase();
    auto facts = c.engine.facts2(0.5f);
    facts.generation = 1;
    CHECK(c.controller.prepare(facts) && c.host.last_request.engine_dsv == &c.engine.objects[61]);
    CHECK(c.controller.seam() && c.stock() && c.host.makes == 3 && c.host.drops == 2);
    // A new HW.pBaseZB alone (no reset counted) also rebuilds, and so does a new back buffer view.
    put<void*>(c.engine.base_zb.data(), 0, &c.engine.objects[62]);
    c.rebase();
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.makes == 4);
    put<void*>(c.engine.base_rt.data(), 0, &c.engine.objects[58]);
    c.rebase();
    CHECK(c.controller.prepare(facts) && c.host.last_request.engine_rtv == &c.engine.objects[58] && c.controller.seam() && c.stock());
    CHECK(c.host.makes == 5);
    // Resolution change.
    c.engine.resize(1280, 720);
    c.engine.init_viewport(640.0f, 360.0f);
    c.rebase();
    facts = c.engine.facts2(0.67f);
    facts.generation = 2;
    CHECK(facts.render_width == 858 && facts.render_height == 482);
    CHECK(c.controller.prepare(facts) && c.controller.twins()[pos].width == 858 && c.controller.twin_set().depth_height == 482);
    CHECK(c.controller.seam() && c.stock() && c.host.makes == 6 && c.host.drops == 5);
    // Switching stages also rebuilds (Stage 1 twins are output-size FP16 copies of generic0/generic_temp only).
    CHECK(c.controller.prepare(c.engine.facts()) && c.host.makes == 7 && c.controller.twins()[g0].width == 1280);
    CHECK(c.controller.seam() && c.stock());
    // Allocation failure at a new shape: stock for that shape, retried when the shape changes.
    c.host.fail_make = true;
    CHECK(!c.controller.prepare(c.engine.facts2(0.6f)) && c.stock() && !c.controller.refused() && skips(c.controller, Skip::Twins) == 1);
    CHECK(!c.controller.prepare(c.engine.facts2(0.6f)) && c.host.makes == 8);   // same key: not retried
    c.host.fail_make = false;
    CHECK(c.controller.prepare(c.engine.facts2(0.7f)) && c.host.makes == 9 && c.controller.seam() && c.stock());
    c.controller.shutdown();
    CHECK(c.stock() && c.controller.counters().twin_drops == uint64_t(c.host.drops));
}

// ---- Stage 3 (jitter and mip bias inside the Stage 2 window) --------------------------------------------------------
// What the first-person sites do between their set_xform_project and rmNear: rebuild Device.mProject with psHUD_FOV
// (_31 = _32 = 0) and mFullTransform = mProject * mView. They restore the saved (jittered) main pair afterwards.
EngineMatrix build_first_person(Engine& e) {
    e.project = Engine::perspective(0.9f, float(e.width) / float(e.height), 0.05f, 1000.0f);
    e.full = engine_mul(e.project, e.view);
    return e.project;
}
bool near(float a, float b, float tolerance = 1e-6f) { return std::fabs(a - b) <= tolerance; }

void stage3_normal() {
    Case c;
    c.engine.init_viewport(960.0f, 540.0f);
    c.rebase();
    const EngineMatrix main = c.engine.project, view = c.engine.view;
    const float user = c.engine.sampler_bias;
    const auto facts = c.engine.facts3();
    const float jx = facts.jitter_x, jy = facts.jitter_y;
    CHECK(c.controller.prepare(facts) && c.controller.jitter_applied() && c.controller.mip_bias_applied());
    // Device.mProject: _31 += 2 jx / W_r * _34, _32 -= 2 jy / H_r * _34 (row vectors); nothing else changes.
    const auto expected = jitter_engine_projection(main, jx, jy, 1286, 724);
    CHECK(expected && c.engine.project == *expected);
    CHECK(near(c.engine.project[8], 2.0f * jx / 1286.0f) && near(c.engine.project[9], -2.0f * jy / 724.0f));
    for (const std::size_t i : {0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15}) CHECK(c.engine.project[i] == main[i]);
    CHECK(c.engine.full == engine_mul(*expected, view) && c.engine.view == view);
    CHECK(c.host.uploaded.size() == 1 && c.host.uploaded[0] == *expected);
    // Mip bias: user + log2(1286 / 1920) - 0.3, through SetMipLODBias.
    const float window = user + float(std::log2(1286.0 / 1920.0) - 0.3);
    CHECK(near(c.engine.sampler_bias, window, 1e-6f) && c.host.biases.size() == 1 && c.controller.window_mip_bias() == c.engine.sampler_bias);
    c.controller.prepared();
    // r_dsgraph_render_hud: a fresh first-person projection gets the same pixel offset once.
    const EngineMatrix saved_main = c.engine.project, saved_full = c.engine.full;
    const auto hud = build_first_person(c.engine);
    c.controller.rm_near();
    const auto hud_jittered = jitter_engine_projection(hud, jx, jy, 1286, 724);
    CHECK(hud_jittered && c.engine.project == *hud_jittered && c.engine.full == engine_mul(*hud_jittered, view));
    CHECK(c.host.uploaded.size() == 2 && c.host.uploaded[1] == *hud_jittered);
    // A second rmNear without a rebuild (no double jitter), then the site's own restore.
    c.controller.rm_near();
    CHECK(c.engine.project == *hud_jittered && c.host.uploaded.size() == 2);
    c.engine.project = saved_main;
    c.engine.full = saved_full;
    // An rmNear while the jittered main projection is current (no rebuild): left alone.
    c.controller.rm_near();
    CHECK(c.engine.project == saved_main && c.host.uploaded.size() == 2);
    // A projection the adapter did not write and that is not freshly built: left alone.
    auto odd = hud;
    odd[8] = 0.01f;
    c.engine.project = odd;
    c.controller.rm_near();
    CHECK(c.engine.project == odd && c.host.uploaded.size() == 2);
    c.engine.project = saved_main;
    // Five more sites in the same window, each rebuilding: each is jittered exactly once.
    for (int site = 0; site < 5; ++site) {
        build_first_person(c.engine);
        c.controller.rm_near();
        CHECK(c.engine.project == *hud_jittered);
        c.engine.project = saved_main;
        c.engine.full = saved_full;
    }
    const auto& n = c.controller.counters();
    CHECK(n.jittered == 1 && n.fp_jittered == 6 && n.fp_already == 2 && n.fp_foreign == 1 && n.rm_near_in_window == 9);
    CHECK(c.controller.seam());
    // The seam restores the stock matrices and bias and re-uploads the unjittered projection before the seam pass.
    CHECK(c.stock() && c.host.uploaded.back() == main && c.host.biases.back() == user);
    CHECK(n.projection_restores == 1 && n.mip_applied == 1 && n.mip_restored == 1 && !c.controller.refused());
    // rmNear outside the window (after the seam): counted nowhere, nothing written.
    c.controller.rm_near();
    CHECK(c.stock() && n.rm_near_in_window == 9);
    c.controller.combine_end();
    c.controller.frame_end();
    CHECK(c.stock() && !c.controller.refused());
    // Next frames with other samples: each window starts from the stock projection (no accumulation).
    for (const auto& s : {jitter_sample(1, 18), jitter_sample(2, 18), jitter_sample(17, 18)}) {
        CHECK(c.controller.prepare(c.engine.facts3(0.67f, s.x, s.y)));
        CHECK(c.engine.project == *jitter_engine_projection(main, s.x, s.y, 1286, 724));
        CHECK(c.controller.seam() && c.stock());
    }
    c.controller.shutdown();
    CHECK(c.stock());
}

void stage3_exits() {
    const EngineMatrix stock_project = Case{}.engine.project;
    // Every non-seam exit restores the matrices and the bias.
    for (int path = 0; path < 4; ++path) {
        Case c;
        const float user = c.engine.sampler_bias;
        CHECK(c.controller.prepare(c.engine.facts3()));
        c.controller.prepared();
        build_first_person(c.engine);
        c.controller.rm_near();   // a first-person site, which then puts the jittered main pair back
        c.engine.project = *jitter_engine_projection(stock_project, 0.25f, -1.0f / 6.0f, 1286, 724);
        c.engine.full = engine_mul(c.engine.project, c.engine.view);
        const int uploads = c.host.upload_calls;
        switch (path) {
        case 0: c.controller.combine_end(); CHECK(restores(c.controller, Restore::CombineEnd) == 1 && c.controller.refused()); break;
        case 1: c.controller.frame_end(); CHECK(restores(c.controller, Restore::FrameEnd) == 1 && c.controller.refused()); break;
        case 2: c.controller.close_stale(); CHECK(restores(c.controller, Restore::StalePrepare) == 1 && !c.controller.refused()); break;
        default:
            // Shutdown: engine memory only for the projection (the next SetCacheXform uploads it again); the bias must
            // still go back through SetMipLODBias, nothing else would ever restore it.
            c.controller.shutdown();
            CHECK(restores(c.controller, Restore::Shutdown) == 1 && c.host.upload_calls == uploads);
            break;
        }
        CHECK(c.stock() && c.engine.sampler_bias == user && c.host.biases.back() == user);
        if (path < 3) CHECK(c.host.upload_calls == uploads + 1 && c.host.uploaded.back() == stock_project);
        CHECK(c.controller.counters().projection_restores == 1 && c.controller.counters().mip_restored == 1);
    }
    // A write fault at each Stage 3 write (Device.mProject, then mFullTransform) rolls the whole window back.
    Case probe;
    probe.host.write_count = 0;
    CHECK(probe.controller.prepare(probe.engine.facts2()));
    const int stage2_writes = probe.host.write_count;
    for (int extra = 0; extra < 2; ++extra) {
        Case c;
        c.host.fail_write_at = stage2_writes + extra;
        CHECK(!c.controller.prepare(c.engine.facts3()));
        CHECK(c.stock() && c.controller.refused() && restores(c.controller, Restore::Rollback) == 1 && !c.controller.window_open());
        CHECK(c.host.upload_calls == 0 && c.host.bias_calls == 0 && !c.controller.jitter_applied());
    }
    {
        // set_P faults: the restore still re-uploads the stock projection.
        Case c;
        c.host.fail_upload_at = 0;
        CHECK(!c.controller.prepare(c.engine.facts3()) && c.stock() && c.controller.refused());
        CHECK(c.host.upload_calls == 2 && c.host.uploaded.size() == 1 && c.host.uploaded[0] == stock_project);
    }
    {
        // SetMipLODBias faults: the restore calls it again with the user's value.
        Case c;
        const float user = c.engine.sampler_bias;
        c.host.fail_bias_at = 0;
        CHECK(!c.controller.prepare(c.engine.facts3()) && c.stock() && c.controller.refused());
        CHECK(c.host.bias_calls == 2 && c.host.biases.size() == 1 && c.host.biases[0] == user);
        CHECK(c.host.uploaded.back() == stock_project);   // the projection, applied first, went back too
    }
    {
        // Restoring the bias faults at the seam: the matrices are back, further swaps refused.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts3()));
        c.host.fail_bias_at = c.host.bias_calls;
        CHECK(c.controller.seam() && c.controller.refused() && c.controller.counters().mip_restored == 0);
        CHECK(c.engine.project == stock_project);
    }
}

void stage3_foreign_and_ineligible() {
    {
        // Something other than a first-person site leaves Device.mProject changed: restored, then refused.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts3()));
        c.engine.project[0] *= 1.01f;
        CHECK(c.controller.seam() && c.stock() && c.controller.refused() && c.controller.counters().foreign_changes == 1);
    }
    {
        // The sampler bias changed inside the window (a console r__tf_mipbias cannot run mid-frame): restored, refused.
        Case c;
        const float user = c.engine.sampler_bias;
        CHECK(c.controller.prepare(c.engine.facts3()));
        c.engine.sampler_bias = 2.0f;
        CHECK(c.controller.seam() && c.engine.sampler_bias == user && c.controller.refused());
    }
    {
        // Scope, MSAA and swap-off frames: no matrix, bias or upload change, rmNear does nothing.
        Case c;
        auto facts = c.engine.facts3();
        facts.scope = true;
        CHECK(!c.controller.prepare(facts) && !c.controller.jitter_applied());
        build_first_person(c.engine);
        c.rebase();
        c.controller.rm_near();
        facts.scope = false;
        facts.msaa = true;
        CHECK(!c.controller.prepare(facts));
        c.controller.rm_near();
        c.controller.set_swap_enabled(false);
        CHECK(!c.controller.prepare(c.engine.facts3()));
        CHECK(c.stock() && c.host.upload_calls == 0 && c.host.bias_calls == 0 && c.controller.counters().fp_jittered == 0);
    }
    {
        // SRJitter=0: mip bias only; rmNear leaves the first-person projection alone.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts3(0.67f, 0.25f, 0.1f, false)) && !c.controller.jitter_applied());
        CHECK(c.controller.mip_bias_applied() && c.host.upload_calls == 0);
        const auto fp = build_first_person(c.engine);
        c.controller.rm_near();
        CHECK(c.engine.project == fp && c.controller.counters().fp_jittered == 0);
        c.engine.project = Case{}.engine.project;
        c.engine.full = Case{}.engine.full;
        CHECK(c.controller.seam() && c.stock());
    }
    {
        // SRMipBias=off, and a delta of exactly zero: SetMipLODBias is never called.
        Case c;
        CHECK(c.controller.prepare(c.engine.facts3(0.67f, 0.25f, 0.1f, true, false)) && !c.controller.mip_bias_applied());
        CHECK(c.controller.seam() && c.stock() && c.host.bias_calls == 0);
        auto facts = c.engine.facts3();
        facts.mip_bias_delta = 0.0f;
        CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.bias_calls == 0);
        CHECK(c.controller.counters().mip_unchanged == 1);
    }
    {
        // A non-finite projection is not jittered (the window still opens) and is put back unchanged.
        Case c;
        c.engine.project[0] = std::numeric_limits<float>::quiet_NaN();
        CHECK(c.controller.prepare(c.engine.facts3()) && !c.controller.jitter_applied() && c.controller.counters().jitter_invalid == 1);
        CHECK(c.host.upload_calls == 0 && c.controller.seam() && !c.controller.refused());
    }
}
}

int main(int argc, char** argv) {
    struct { const char* name; void (*run)(); } cases[] = {{"normal", normal_frame}, {"blur_only", blur_only_frame},
        {"combine_without_seam", combine_without_seam}, {"stale_prepare", stale_prepare}, {"frame_end_open", frame_end_open},
        {"shutdown_open", shutdown_open}, {"ineligible", ineligible_frames}, {"layout", layout_mismatch},
        {"twin_failures", twin_failures}, {"write_fault", write_fault_rolls_back}, {"foreign_change", foreign_change},
        {"round_trip_failure", round_trip_failure}, {"reset_resize", reset_and_resize}, {"names", names},
        {"stage2_normal", stage2_normal}, {"stage2_optional_odd", stage2_optional_and_odd}, {"stage2_layout", stage2_layout},
        {"stage2_exits", stage2_exits}, {"stage2_foreign", stage2_foreign}, {"stage2_ssao_viewport", stage2_ssao_viewport},
        {"stage2_write_faults", stage2_write_faults}, {"stage2_lifecycle", stage2_lifecycle}, {"stage3_normal", stage3_normal},
        {"stage3_exits", stage3_exits}, {"stage3_foreign_ineligible", stage3_foreign_and_ineligible}};
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
