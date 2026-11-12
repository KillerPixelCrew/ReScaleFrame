// SPDX-License-Identifier: GPL-3.0-only
// SR Stage 2 render/output plan arithmetic (games/anomaly/src/sr_plan.h): scale clamping, per-axis rounding, odd and
// tiny sizes, the minimum/maximum scale, and the half-size rule of rt_half_depth. No game, no device.
#include "sr_plan.h"

#include <cmath>
#include <cstdio>
#include <initializer_list>
#include <limits>

namespace {
using namespace rsf::anomaly::sr;
int failures = 0;
#define CHECK(condition) do { if (!(condition)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #condition); ++failures; } } while (0)

void scales() {
    CHECK(clamp_scale(std::numeric_limits<float>::quiet_NaN()) == default_scale);
    CHECK(clamp_scale(std::numeric_limits<float>::infinity()) == default_scale);
    CHECK(clamp_scale(-1.0f) == default_scale && clamp_scale(0.0f) == default_scale);
    CHECK(clamp_scale(0.3f) == min_scale && clamp_scale(0.5f) == 0.5f && clamp_scale(0.67f) == 0.67f);
    CHECK(clamp_scale(1.0f) == 1.0f && clamp_scale(1.5f) == max_scale && clamp_scale(100.0f) == max_scale);
    CHECK(default_scale == 0.67f && min_scale == 0.5f && max_scale == 1.0f);
}

void rounding() {
    // Round half up per axis; one scale for both axes.
    CHECK(render_extent(1920, 0.67f) == 1286 && render_extent(1080, 0.67f) == 724);   // 1286.4, 723.6
    CHECK(render_extent(2560, 0.67f) == 1715 && render_extent(1440, 0.67f) == 965);   // 1715.2, 964.8
    CHECK(render_extent(3840, 0.77f) == 2957 && render_extent(2160, 0.77f) == 1663);  // 2956.8, 1663.2
    CHECK(render_extent(1280, 0.67f) == 858 && render_extent(720, 0.67f) == 482);     // 857.6, 482.4
    // Odd outputs.
    CHECK(render_extent(1919, 0.67f) == 1286 && render_extent(1079, 0.67f) == 723);   // 1285.73, 722.93
    CHECK(render_extent(1921, 0.5f) == 961 && render_extent(1081, 0.5f) == 541);     // x.5 rounds up, = ceil(output/2)
    // Limits: 0.5 is exactly half (rounded up), 1.0 is the output, nothing below 1.
    CHECK(render_extent(1920, 0.5f) == 960 && render_extent(1920, 1.0f) == 1920 && render_extent(1920, 0.3f) == 960);
    CHECK(render_extent(1, 0.5f) == 1 && render_extent(2, 0.5f) == 1 && render_extent(3, 0.5f) == 2 && render_extent(0, 0.67f) == 0);
    CHECK(render_extent(1, 0.67f) == 1 && render_extent(7, 2.0f) == 7);
    // Every size and a range of scales: inside [ceil(output/2), output], within half a pixel of output * scale.
    int bad = 0;
    for (const float scale : {0.5f, 0.55f, 0.6f, 0.6667f, 0.67f, 0.75f, 0.77f, 0.9f, 1.0f})
        for (uint32_t output = 1; output <= 8192; ++output) {
            const auto render = render_extent(output, scale);
            const double exact = double(output) * double(scale);
            if (render < 1 || render > output || render < uint32_t(std::ceil(output * 0.5)) || std::fabs(double(render) - exact) > 0.5 + 1e-9)
                ++bad;
        }
    CHECK(bad == 0);
}

void plans() {
    ScalePlan plan{};
    CHECK(!make_plan(0, 1080, 0.67f, plan) && !make_plan(1920, 0, 0.67f, plan));
    CHECK(make_plan(1920, 1080, 0.67f, plan) && plan.render_width == 1286 && plan.render_height == 724 && plan.scale == 0.67f &&
        !plan.identity());
    CHECK(make_plan(1920, 1080, 1.0f, plan) && plan.identity());
    CHECK(make_plan(1920, 1080, 7.0f, plan) && plan.identity() && plan.scale == 1.0f);   // clamped
    CHECK(make_plan(1920, 1080, std::numeric_limits<float>::quiet_NaN(), plan) && plan.scale == default_scale && plan.render_width == 1286);
    CHECK(make_plan(1, 1, 0.5f, plan) && plan.render_width == 1 && plan.render_height == 1 && plan.identity());
}

void extents() {
    uint32_t twin{};
    Extent rule{};
    CHECK(twin_extent(1920, 1920, 1286, false, twin, rule) && twin == 1286 && rule == Extent::Full);
    CHECK(twin_extent(960, 1920, 1286, true, twin, rule) && twin == 643 && rule == Extent::Half);
    CHECK(!twin_extent(960, 1920, 1286, false, twin, rule) && twin == 0);    // half not allowed for this target
    CHECK(twin_extent(959, 1919, 1286, true, twin, rule) && twin == 643);     // engine integer division of an odd output
    CHECK(twin_extent(539, 1079, 723, true, twin, rule) && twin == 361);
    CHECK(!twin_extent(480, 1920, 1286, true, twin, rule));                 // quarter size: not a known rule
    CHECK(!twin_extent(0, 1920, 1286, true, twin, rule) && !twin_extent(1920, 0, 1286, true, twin, rule));
    CHECK(!twin_extent(1920, 1920, 2000, true, twin, rule));               // render larger than output
    CHECK(!twin_extent(0, 1, 1, true, twin, rule) && twin_extent(1, 1, 1, true, twin, rule) && twin == 1);
    CHECK(!twin_extent(1, 3, 1, true, twin, rule));                         // half of a 1-pixel render is empty
    CHECK(twin_extent(1920, 1920, 1920, true, twin, rule) && twin == 1920 && rule == Extent::Full);   // scale 1.0
    CHECK(twin_extent(960, 1920, 1920, true, twin, rule) && twin == 960 && rule == Extent::Half);
}
}

int main() {
    scales();
    rounding();
    plans();
    extents();
    std::printf("%d failure(s)\n", failures);
    return failures ? 1 : 0;
}
