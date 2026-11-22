// SPDX-License-Identifier: MIT
#include "screen.h"
#include <stddef.h>

extern "C" {
struct rsf_fallout4_menu_call { uintptr_t name, expected; };
struct rsf_fallout4_screen_plan {
    uintptr_t ui;
    rsf_fallout4_menu_call menus[6];
    uintptr_t menu_open, freeze_background, freeze_pause, camera_equals, camera;
};
struct rsf_fallout4_screen_result {
    uintptr_t names[6];
    uint8_t open[6], frozen_background, frozen_pause;
    uint16_t cameras;
};
using ScreenObserve = bool (*)(const rsf_fallout4_screen_plan&, rsf_fallout4_screen_result&);
void rsf_fallout4_screen_classify(const rsf_fallout4_cpu_identity*, const rsf_game_render_pass*,
    bool ordinary_world, rsf::fallout4::NativeScreen*, ScreenObserve) noexcept;
}

static_assert(sizeof(rsf_fallout4_screen_plan) == 144 && offsetof(rsf_fallout4_screen_plan, menus) == 8);
static_assert(sizeof(rsf_fallout4_screen_result) == 64 && offsetof(rsf_fallout4_screen_result, cameras) == 56);
static_assert(sizeof(rsf::fallout4::NativeScreen) == 24 && offsetof(rsf::fallout4::NativeScreen, camera_state) == 16);
static_assert(RSF_SCREEN_UNKNOWN == 0 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_LOADING == 7 && RSF_SCREEN_GAMEPLAY == 8);

namespace rsf::fallout4 {
namespace {
using MenuName = const void* (*)();
using MenuOpen = bool (*)(void*, const void*);
using Freeze = bool (*)(void*);
using CameraEquals = bool (*)(void*, uint32_t);

bool observe(const rsf_fallout4_screen_plan& plan, rsf_fallout4_screen_result& observed) noexcept try
{
    auto* ui = reinterpret_cast<void*>(plan.ui);
    const auto open = reinterpret_cast<MenuOpen>(plan.menu_open);
    for (size_t index = 0; index < 6; ++index) {
        const auto name = reinterpret_cast<MenuName>(plan.menus[index].name)();
        observed.names[index] = uintptr_t(name);
        // Only the exact verified menu object can enter the native UI query.
        if (observed.names[index] != plan.menus[index].expected) return true;
        observed.open[index] = static_cast<uint8_t>(open(ui, name));
    }
    observed.frozen_background = static_cast<uint8_t>(reinterpret_cast<Freeze>(plan.freeze_background)(ui));
    observed.frozen_pause = static_cast<uint8_t>(reinterpret_cast<Freeze>(plan.freeze_pause)(ui));
    if (plan.camera) {
        const auto equals = reinterpret_cast<CameraEquals>(plan.camera_equals);
        for (uint32_t index = 0; index < 13; ++index) {
            if (equals(reinterpret_cast<void*>(plan.camera), index))
                observed.cameras = static_cast<uint16_t>(observed.cameras | (1u << index));
        }
    }
    return true;
} catch (...) { return false; }
}

NativeScreen classify_native_screen(const CpuIdentity& source, const rsf_game_render_pass& pass,
    bool ordinary_world) noexcept
{
    const auto identity = cpu_identity_abi(source);
    NativeScreen result{};
    rsf_fallout4_screen_classify(&identity, &pass, ordinary_world, &result, observe);
    return result;
}
}
