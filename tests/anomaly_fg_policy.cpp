// SPDX-License-Identifier: GPL-3.0-only
// games/anomaly/src/fg_policy.h without the engine: which frames feed generation history, which are held back, why,
// and when history restarts. Every engine frame is counted under exactly one skip reason (None: fed).
#include "fg_policy.h"
#include <cstdio>
#include <cstring>
#include <numeric>
#include <string_view>

namespace {
using namespace rsf::anomaly::fg;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

struct Frame {
    bool level{true}, scope{}, presented{true}, inputs{true}, cut{};
    uint32_t width{1280}, height{720};
};
struct Run {
    Policy policy{4};
    uint32_t engine_frame{100};
    Decision begin_decision{}, last{};
    Decision step(const Frame& f) {
        begin_decision = policy.begin(++engine_frame, f.width, f.height, f.cut);
        if (f.scope) policy.scope_frame();
        if (f.level) {
            const auto d = policy.ui_boundary();
            if (d.tag) policy.inputs(f.inputs);
        }
        if (f.presented && !f.scope) policy.presented();
        last = policy.end();
        return last;
    }
};

void continuous_gameplay() {
    Run r;
    auto d = r.step({});
    CHECK(d.skip == Skip::None && d.reset == Reset::First && d.screen == gameplay_screen && r.begin_decision.tag);
    d = r.step({});
    CHECK(d.skip == Skip::None && d.reset == Reset::None);
    const auto& c = r.policy.counters();
    CHECK(c.source_frames == 2 && c.level_frames == 2 && c.tagged == 2 && c.fed == 2 && c.presented == 2);
    CHECK(c.resets[size_t(Reset::First)] == 1 && c.resets[size_t(Reset::None)] == 1);
}

void menu_and_loading_break_history() {
    Run r;
    r.step({}); r.step({});
    Frame menu{}; menu.level = false;   // the main menu removes the level from seqRender: no UI boundary
    for (int i = 0; i < 3; ++i) CHECK(r.step(menu).skip == Skip::NoLevel);
    Frame loading{}; loading.level = false;
    CHECK(r.step(loading).skip == Skip::NoLevel);
    const auto d = r.step({});
    CHECK(d.skip == Skip::None && d.reset == Reset::Discontinuity);
    CHECK(r.policy.counters().skipped[size_t(Skip::NoLevel)] == 4);
}

void scope_frames_never_feed_history() {
    Run r;
    r.step({}); r.step({});
    // Every other engine frame is a secondary-viewport frame: RenderToTarget(rtSVP), then no Present.
    for (int i = 0; i < 10; ++i) {
        Frame f{}; f.scope = (i % 2) == 0;
        const auto d = r.step(f);
        CHECK(d.skip == Skip::Scope);
        CHECK(!f.scope || !r.begin_decision.tag || r.begin_decision.skip == Skip::Scope || i == 0);
        if (!f.scope) CHECK(r.begin_decision.screen == held_screen);
    }
    CHECK(r.policy.counters().scope_frames == 5);
    CHECK(r.policy.counters().unpresented == 5);
    // The scope closed: frames inside the hold window stay held, then history restarts with ScopeEnd.
    uint32_t held = 0;
    Decision d{};
    for (int i = 0; i < 8; ++i) {
        d = r.step({});
        if (d.skip == Skip::Scope) ++held;
        else break;
    }
    CHECK(held == 3);   // the last SVP frame was 2 engine frames before the first of these; hold is 4
    CHECK(d.skip == Skip::None && d.reset == Reset::ScopeEnd);
    CHECK(r.step({}).reset == Reset::None);
}

void resets() {
    Run r;
    r.step({}); r.step({});
    r.policy.device_reset();
    CHECK(r.step({}).reset == Reset::Device);
    Frame bigger{}; bigger.width = 1600; bigger.height = 900;
    CHECK(r.step(bigger).reset == Reset::Size);
    CHECK(r.step(bigger).reset == Reset::None);
    Frame cut = bigger; cut.cut = true;
    CHECK(r.step(cut).reset == Reset::Cut);
    // A device reset during a held run is not lost: it is the restart reason once frames feed again.
    r.policy.device_reset();
    Frame no_inputs = bigger; no_inputs.inputs = false;
    CHECK(r.step(no_inputs).skip == Skip::NoInputs);
    CHECK(r.step(bigger).reset == Reset::Device);
}

void unpresented_and_missing_inputs() {
    Run r;
    r.step({});
    Frame refused{}; refused.presented = false;
    CHECK(r.step(refused).skip == Skip::NoPresent);
    CHECK(r.step({}).reset == Reset::Discontinuity);
    Frame missing{}; missing.inputs = false;
    CHECK(r.step(missing).skip == Skip::NoInputs);
    CHECK(r.step({}).reset == Reset::Discontinuity);
}

void every_frame_counted_once() {
    Run r;
    const Frame pattern[] = {{}, {}, {false}, {}, {true, true}, {}, {}, {}, {}, {}, {true, false, false}, {}};
    for (int k = 0; k < 20; ++k) for (const auto& f : pattern) r.step(f);
    const auto& c = r.policy.counters();
    const auto skipped = std::accumulate(c.skipped.begin(), c.skipped.end(), uint64_t{});
    CHECK(skipped == c.source_frames);
    CHECK(c.skipped[size_t(Skip::None)] == c.fed);
    CHECK(c.presented + c.unpresented == c.source_frames);
    const auto resets = std::accumulate(c.resets.begin(), c.resets.end(), uint64_t{});
    CHECK(resets == c.fed);
}
}

int main(int argc, char** argv) {
    const std::string_view selected = argc > 1 ? argv[1] : "";
    struct Case { const char* name; void (*run)(); } cases[] = {
        {"continuous", continuous_gameplay}, {"menu", menu_and_loading_break_history},
        {"scope", scope_frames_never_feed_history}, {"resets", resets},
        {"unpresented", unpresented_and_missing_inputs}, {"counted", every_frame_counted_once}};
    bool ran = false;
    for (const auto& c : cases) if (selected.empty() || selected == c.name) { c.run(); ran = true; }
    if (!ran) { std::printf("unknown case %.*s\n", int(selected.size()), selected.data()); return 2; }
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::puts("PASS: Anomaly FG frame policy");
    return 0;
}
