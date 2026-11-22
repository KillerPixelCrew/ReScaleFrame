// SPDX-License-Identifier: MIT
#pragma once
#include "render_records.h"
#include <rescaleframe/game_renderer.h>
RSF_FALLOUT4_RENDER_ABI_BEGIN
// Called only in an engine-owned callback with the reader's synchronous world
// resource/camera lease. ordinary_world is the guarded parent C382A5 fact.
// This returns facts only; it neither clears NO_FG nor admits an input owner.
void rsf_fallout4_screen_classify(const rsf_fallout4_cpu_identity*,
    const rsf_game_render_pass*, bool ordinary_world, NativeScreen*) RSF_FALLOUT4_RENDER_NOEXCEPT;

// Refusal facts for the ordinary World before a render-input lease is available.
uint32_t rsf_fallout4_ordinary_screen_refusals(const rsf_fallout4_cpu_identity*) RSF_FALLOUT4_RENDER_NOEXCEPT;

RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativeScreen) == 24 && offsetof(NativeScreen, camera_state) == 16);
RSF_FALLOUT4_RENDER_ASSERT(RSF_SCREEN_UNKNOWN == 0 && RSF_SCREEN_MENU == 1 && RSF_SCREEN_LOADING == 7 && RSF_SCREEN_GAMEPLAY == 8);

// The two engine routines remain behind the native exception guards.
bool rsf_fallout4_menu_name(uintptr_t entry, uintptr_t* name) RSF_FALLOUT4_RENDER_NOEXCEPT;
bool rsf_fallout4_menu_open(uintptr_t entry, uintptr_t ui, uintptr_t name,
    bool* open) RSF_FALLOUT4_RENDER_NOEXCEPT;
RSF_FALLOUT4_RENDER_ABI_END
