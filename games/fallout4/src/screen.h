// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events_platform.h"
#include "render_records.h"
#include <cstddef>
#include <rescaleframe/game_renderer.h>
namespace rsf::fallout4 {
// Called only in an engine-owned callback with the reader's synchronous world
// resource/camera lease. ordinary_world is the guarded parent C382A5 fact.
// This returns facts only; it neither clears NO_FG nor admits an input owner.
extern "C" void rsf_fallout4_screen_classify(const rsf_fallout4_cpu_identity*,
    const rsf_game_render_pass*, bool ordinary_world, NativeScreen*) noexcept;

static_assert(sizeof(NativeScreen) == 24 && offsetof(NativeScreen, camera_state) == 16);
static_assert(RSF_SCREEN_UNKNOWN == 0 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_LOADING == 7 && RSF_SCREEN_GAMEPLAY == 8);
}
