// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 1 swap/restore state machine on fake CRenderTarget / CRT / CTexture / RCache memory laid out with the
// offsets of games/anomaly/src/sr_profile.h. Every exit path must leave the engine bytes exactly as they were.
// No game code, D3D device or executable is involved.
#include "sr_profile.h"
#include "sr_swap.h"

#include <array>
#include <cstdio>
#include <cstring>
#include <string_view>
#include <vector>

namespace {
using namespace rsf::anomaly;
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

template<class T> void put(std::byte* base, uint32_t offset, T value) { std::memcpy(base + offset, &value, sizeof(T)); }
template<class T> T get(const std::byte* base, uint32_t offset) { T value; std::memcpy(&value, base + offset, sizeof(T)); return value; }

struct Engine {
    alignas(16) std::array<std::byte, 0x600> target{};
    alignas(16) std::array<std::array<std::byte, crt_size>, 2> crt{};
    alignas(16) std::array<std::array<std::byte, 0xb0>, 2> texture{};
    alignas(16) std::array<std::byte, 0x900> backend{};
    std::array<int, 16> objects{};   // addresses stand in for D3D objects
    uint32_t width = 1920, height = 1080;
    Engine() {
        for (std::size_t i = 0; i < 2; ++i) {
            auto* c = crt[i].data();
            auto* t = texture[i].data();
            std::memset(c, 0xa0 + int(i), crt_size);           // reference count / order / padding patterns
            std::memset(t, 0xc0 + int(i), texture[i].size());
            put<void*>(c, crt_surface_offset, &objects[i * 4 + 0]);
            put<void*>(c, crt_rtv_offset, &objects[i * 4 + 1]);
            put<void*>(c, crt_dsv_offset, nullptr);
            put<void*>(c, crt_uav_offset, i == 0 ? &objects[i * 4 + 3] : nullptr);
            put<uint32_t>(c, crt_width_offset, width);
            put<uint32_t>(c, crt_height_offset, height);
            put<uint32_t>(c, crt_format_offset, d3dfmt_a8r8g8b8);
            put<void*>(c, crt_texture_offset, t);
            put<void*>(t, texture_surface_offset, &objects[i * 4 + 0]);
            put<void*>(t, texture_desc_cache_offset, &objects[i * 4 + 0]);
            put<void*>(t, texture_srv_offset, &objects[i * 4 + 2]);
        }
        put<void*>(target.data(), 0x3a8, crt[0].data());
        put<void*>(target.data(), 0x3b8, crt[1].data());
        std::memset(backend.data(), 0x5a, backend.size());
    }
    void resize(uint32_t w, uint32_t h) {
        width = w; height = h;
        for (auto& c : crt) { put<uint32_t>(c.data(), crt_width_offset, w); put<uint32_t>(c.data(), crt_height_offset, h); }
    }
    FrameFacts facts() { return {target.data(), backend.data(), 0x1234, 0, width, height, false, false}; }
    std::vector<std::byte> snapshot() const {
        std::vector<std::byte> bytes;
        for (const auto& c : crt) bytes.insert(bytes.end(), c.begin(), c.end());
        for (const auto& t : texture) bytes.insert(bytes.end(), t.begin(), t.end());
        bytes.insert(bytes.end(), target.begin(), target.end());
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
};

struct FakeHost final : SwapHost {
    std::array<int, 16> twin_objects{};
    int write_count = 0, fail_write_at = -1;
    bool fail_make = false, fail_round_trip = false, wrong_format = false;
    int makes = 0, drops = 0, round_trips = 0;
    TargetFields last_twin{}, last_engine{};
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
    bool make_twins(const std::array<EngineTarget, slot_count>& e, std::array<TargetFields, slot_count>& out) noexcept override {
        ++makes;
        if (fail_make) return false;
        for (std::size_t i = 0; i < slot_count; ++i) {
            auto& f = out[i];
            f.surface = f.texture_surface = f.desc_cache = &twin_objects[i * 4 + 0];
            f.rtv = &twin_objects[i * 4 + 1];
            f.srv = &twin_objects[i * 4 + 2];
            f.uav = e[i].fields.uav ? &twin_objects[i * 4 + 3] : nullptr;
            f.width = e[i].fields.width;
            f.height = e[i].fields.height;
            f.format = wrong_format ? d3dfmt_a8r8g8b8 : d3dfmt_a16b16g16r16f;
            f.desc.fill(std::byte{0x77});
        }
        return true;
    }
    void drop_twins() noexcept override { ++drops; }
    bool round_trip(const TargetFields& twin, const TargetFields& original) noexcept override {
        ++round_trips;
        last_twin = twin;
        last_engine = original;
        if (engine) engine_at_round_trip = engine->snapshot();
        return !fail_round_trip;
    }
};

bool engine_points_at_twins(const Engine& engine, const SwapController& controller) {
    for (std::size_t i = 0; i < 2; ++i) {
        const auto& twin = controller.twins()[i];
        const auto* c = engine.crt[i].data();
        const auto* t = engine.texture[i].data();
        if (get<void*>(c, crt_surface_offset) != twin.surface || get<void*>(c, crt_rtv_offset) != twin.rtv ||
            get<void*>(c, crt_uav_offset) != twin.uav || get<uint32_t>(c, crt_format_offset) != d3dfmt_a16b16g16r16f ||
            get<void*>(t, texture_surface_offset) != twin.surface || get<void*>(t, texture_desc_cache_offset) != twin.surface ||
            get<void*>(t, texture_srv_offset) != twin.srv || get<uint8_t>(t, texture_desc_offset) != 0x77)
            return false;
        // The CRT reference count, CTexture pointer and order are never exchanged.
        if (get<void*>(c, crt_texture_offset) != t || get<uint8_t>(c, crt_reference_offset) != 0xa0 + i ||
            get<uint8_t>(c, crt_order_offset) != 0xa0 + i || get<uint8_t>(t, texture_reference_offset) != 0xc0 + i)
            return false;
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
    Case() { host.engine = &engine; controller.set_swap_enabled(true); original = engine.snapshot(); }
    bool stock() const { return engine.snapshot() == original; }
};

void normal_frame() {
    Case c;
    CHECK(c.controller.prepare(c.engine.facts()));
    CHECK(c.controller.window_open() && engine_points_at_twins(c.engine, c.controller) && c.engine.texture_cache_cleared());
    c.controller.rm_near();
    std::memset(c.engine.backend.data(), 0x5a, c.engine.backend.size());
    CHECK(c.controller.seam());                     // phase_sunshafts: the seam
    CHECK(c.stock() && !c.controller.window_open() && c.engine.texture_cache_cleared());
    CHECK(c.host.round_trips == 1 && c.host.engine_at_round_trip == c.original);   // the pass runs on restored fields
    CHECK(c.host.last_twin == c.controller.twins()[0] && c.host.last_engine.surface == &c.engine.objects[0] &&
        c.host.last_engine.rtv == &c.engine.objects[1]);
    CHECK(!c.controller.seam());                    // phase_blur in the same frame passes through
    c.controller.combine_end();
    c.controller.frame_end();
    c.controller.rm_near();                         // outside the window: not counted
    CHECK(c.stock() && !c.controller.refused());
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
        c.original = c.engine.snapshot();
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
        Case c;
        c.host.fail_make = true;
        CHECK(!c.controller.prepare(c.engine.facts()) && c.stock() && c.controller.refused() && skips(c.controller, Skip::Twins) == 1);
    }
    {
        Case c;
        c.host.wrong_format = true;   // a twin that is not FP16 is refused and dropped
        CHECK(!c.controller.prepare(c.engine.facts()) && c.stock() && c.controller.refused() && c.host.drops == 1);
    }
}

void write_fault_rolls_back() {
    // Fail at every write position of the exchange (11 field writes per slot) and of the cache invalidation.
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
    c.original = c.engine.snapshot();
    facts = c.engine.facts();
    facts.generation = 2;
    CHECK(c.controller.prepare(facts) && engine_points_at_twins(c.engine, c.controller));
    CHECK(c.controller.twins()[0].width == 1280 && c.controller.twins()[1].height == 720);
    CHECK(c.controller.seam() && c.stock() && c.host.makes == 3 && c.host.drops == 2);
    // A recreated CRT (new surface address) at the same generation also rebuilds.
    put<void*>(c.engine.crt[0].data(), crt_surface_offset, &c.engine.objects[14]);
    put<void*>(c.engine.texture[0].data(), texture_surface_offset, &c.engine.objects[14]);
    put<void*>(c.engine.texture[0].data(), texture_desc_cache_offset, nullptr);   // desc not yet cached
    c.original = c.engine.snapshot();
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.makes == 4);
    // The device changes (a new device after a full restart).
    facts.device = 0x5678;
    CHECK(c.controller.prepare(facts) && c.controller.seam() && c.stock() && c.host.makes == 5);
}

void names() {
    CHECK(std::string_view{skip_name(Skip::Scope)} == "scope" && std::string_view{skip_name(Skip::Count)} == "unknown");
    CHECK(std::string_view{restore_name(Restore::FrameEnd)} == "frame_end");
}
}

int main(int argc, char** argv) {
    struct { const char* name; void (*run)(); } cases[] = {{"normal", normal_frame}, {"blur_only", blur_only_frame},
        {"combine_without_seam", combine_without_seam}, {"stale_prepare", stale_prepare}, {"frame_end_open", frame_end_open},
        {"shutdown_open", shutdown_open}, {"ineligible", ineligible_frames}, {"layout", layout_mismatch},
        {"twin_failures", twin_failures}, {"write_fault", write_fault_rolls_back}, {"foreign_change", foreign_change},
        {"round_trip_failure", round_trip_failure}, {"reset_resize", reset_and_resize}, {"names", names}};
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
