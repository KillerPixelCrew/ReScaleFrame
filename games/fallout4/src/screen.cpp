// SPDX-License-Identifier: MIT
// Contain exceptions from the two native menu query signatures.
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
