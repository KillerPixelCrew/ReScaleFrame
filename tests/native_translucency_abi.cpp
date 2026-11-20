// SPDX-License-Identifier: MIT
// CPU-only refusal contract for the translucency entry points, linked once against the Runtime DLL
// and once against the embedded core. No device is created, so every call is refused before any
// graphics work and nothing is published.
#include <rescaleframe/native_translucency.h>
#include <cstddef>
#include <cstdio>
#include <initializer_list>

static_assert(sizeof(rsf_native_translucency_masks) == 56, "translucency mask ABI");
static_assert(offsetof(rsf_native_translucency_masks, motion_depth) == 48, "translucency mask ABI");

namespace {
bool ok = true;
void check(bool condition, const char* what)
{
    if (!condition) { std::fprintf(stderr, "native translucency ABI: %s\n", what); ok = false; }
}
rsf_game_render_pass pass(uint32_t role)
{
    rsf_game_render_pass p{};
    p.struct_size = sizeof(p); p.role = role; p.family_key = 1; p.view_key = 2; p.native_frame = 3;
    p.flags = RSF_GAME_RENDER_PRIMARY;
    p.render_rect[2] = 8; p.render_rect[3] = 4;
    return p;
}
}

int main()
{
    auto sr = pass(RSF_GAME_RENDER_SR);
    rsf_native_translucency_masks masks{};
    masks.struct_size = sizeof(masks);
    check(rsf_native_translucency_take(nullptr, &masks) == 0, "a null SR pass must be refused");
    check(rsf_native_translucency_take(&sr, nullptr) == 0, "a null output must be refused");
    masks.struct_size = sizeof(masks) - 1;
    check(rsf_native_translucency_take(&sr, &masks) == 0, "a short output must be refused");
    masks.struct_size = sizeof(masks);
    auto short_sr = sr; short_sr.struct_size = sizeof(sr) - 1;
    check(rsf_native_translucency_take(&short_sr, &masks) == 0, "a short SR pass must be refused");
    check(rsf_native_translucency_take(&sr, &masks) == 0, "nothing is published before a scope");
    check(!masks.reactive && !masks.coverage && !masks.bias && !masks.color_before_transparency &&
        !masks.transparency_layer && !masks.motion_depth, "a refusal must not write the output");

    // Without a context every role is ignored, the material bias included.
    for (uint32_t role : {RSF_GAME_RENDER_TRANSLUCENCY, RSF_GAME_RENDER_CLOUD_DEPTH, RSF_GAME_RENDER_MATERIALS}) {
        auto p = pass(role);
        rsf_native_translucency_pass(nullptr, &p, 1);
        rsf_native_translucency_pass(nullptr, &p, 0);
    }
    rsf_native_translucency_pass(nullptr, nullptr, 1);
    check(rsf_native_translucency_take(&sr, &masks) == 0, "ignored passes must not publish");

    rsf_native_translucency_release();
    rsf_native_translucency_release();
    check(rsf_native_translucency_take(&sr, &masks) == 0, "nothing is published after release");
    std::puts(ok ? "native_translucency_abi passed" : "native_translucency_abi FAILED");
    return ok ? 0 : 1;
}
