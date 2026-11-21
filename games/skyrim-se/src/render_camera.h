// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_renderer.h>
#include <stddef.h>

namespace rsf::skyrim {
struct RenderCamera {
    uintptr_t camera, window;
    uint32_t frame;
    uint32_t output[2], render[2];
    float ratios[2];
};
static_assert(sizeof(RenderCamera) == 48 && offsetof(RenderCamera, frame) == 16 && offsetof(RenderCamera, ratios) == 36);

bool read_render_camera(void* manager, bool fg, rsf_game_render_pass&, RenderCamera&) noexcept;
}
