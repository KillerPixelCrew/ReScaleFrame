// SPDX-License-Identifier: MIT
#pragma once
#include "cpu_events_platform.h"
#include <cstddef>
#include <rescaleframe/game_renderer.h>
namespace rsf::fallout4 {
struct NativeScreen {
    uint32_t screen{RSF_SCREEN_UNKNOWN}, facts{}, refusals{};
    uint32_t camera_index{UINT32_MAX};
    uintptr_t camera_state{};
};
// Called only in an engine-owned callback with the reader's synchronous world
// resource/camera lease. ordinary_world is the guarded parent C382A5 fact.
// This returns facts only; it neither clears NO_FG nor admits an input owner.
extern "C" void rsf_fallout4_screen_classify(const rsf_fallout4_cpu_identity*,
    const rsf_game_render_pass*, bool ordinary_world, NativeScreen*) noexcept;

inline NativeScreen classify_native_screen(const CpuIdentity& source, const rsf_game_render_pass& pass,
    bool ordinary_world) noexcept {
    NativeScreen result{};
    rsf_fallout4_screen_classify(&source, &pass, ordinary_world, &result);
    return result;
}

static_assert(sizeof(NativeScreen) == 24 && offsetof(NativeScreen, camera_state) == 16);
static_assert(RSF_SCREEN_UNKNOWN == 0 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_LOADING == 7 && RSF_SCREEN_GAMEPLAY == 8);
}
