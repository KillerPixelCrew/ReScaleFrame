// SPDX-License-Identifier: MIT
#include "native_camera.h"
#include "native_read.h"

extern "C" bool rsf_wingman_read_camera(const void* view, rsf_game_render_pass* pass,
    bool (*copy)(void*, const void*, size_t)) noexcept;

namespace rsf::wingman {
bool read_camera(const void* view, rsf_game_render_pass& pass) noexcept
{
    return rsf_wingman_read_camera(view, &pass, copy);
}
}
