/* SPDX-License-Identifier: MIT */
/* Contain foreign C++ exceptions from the two native menu query signatures. */
#include "screen.h"
#include "native_guard.h"

typedef const void* (*MenuName)(void);
typedef bool (*MenuOpen)(void*, const void*);

bool rsf_fallout4_menu_name(uintptr_t entry, uintptr_t* name)
{
    bool returned = true;
    RSF_FALLOUT4_TRY { *name = (uintptr_t)((MenuName)entry)(); }
    RSF_FALLOUT4_CATCH_CXX { returned = false; }
    return returned;
}

bool rsf_fallout4_menu_open(uintptr_t entry, uintptr_t ui, uintptr_t name, bool* open)
{
    bool returned = true;
    RSF_FALLOUT4_TRY { *open = ((MenuOpen)entry)((void*)ui, (const void*)name); }
    RSF_FALLOUT4_CATCH_CXX { returned = false; }
    return returned;
}
