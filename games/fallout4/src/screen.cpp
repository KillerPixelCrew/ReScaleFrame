// SPDX-License-Identifier: MIT
// Contain exceptions from the four native screen query signatures.
#include "screen.h"

extern "C" bool rsf_fallout4_menu_name(uintptr_t entry, uintptr_t* name) noexcept try {
    using MenuName = const void* (*)();
    *name = reinterpret_cast<uintptr_t>(reinterpret_cast<MenuName>(entry)());
    return true;
} catch (...) { return false; }

extern "C" bool rsf_fallout4_menu_open(uintptr_t entry, uintptr_t ui, uintptr_t name, bool* open) noexcept try {
    using MenuOpen = bool (*)(void*, const void*);
    *open = reinterpret_cast<MenuOpen>(entry)(reinterpret_cast<void*>(ui), reinterpret_cast<const void*>(name));
    return true;
} catch (...) { return false; }

extern "C" bool rsf_fallout4_menu_freeze(uintptr_t entry, uintptr_t ui, bool* frozen) noexcept try {
    using Freeze = bool (*)(void*);
    *frozen = reinterpret_cast<Freeze>(entry)(reinterpret_cast<void*>(ui));
    return true;
} catch (...) { return false; }

extern "C" bool rsf_fallout4_camera_equals(uintptr_t entry, uintptr_t camera, uint32_t index, bool* equal) noexcept try {
    using CameraEquals = bool (*)(void*, uint32_t);
    *equal = reinterpret_cast<CameraEquals>(entry)(reinterpret_cast<void*>(camera), index);
    return true;
} catch (...) { return false; }
