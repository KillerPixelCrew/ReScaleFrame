// SPDX-License-Identifier: MIT
#include "social_club.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <MinHook.h>
#include <cstdint>

struct SocialClubPlatform {
    int32_t (*create_dispatch)(uintptr_t);
    int32_t (*create_trace)(uintptr_t);
    int32_t (*enable)(uintptr_t);
    bool (*current_index)(void*, uint32_t*);
};
static_assert(sizeof(SocialClubPlatform) == 4 * sizeof(void*));
static_assert(MH_ERROR_ENABLED == 5 && MH_ERROR_MEMORY_PROTECT == 10);

extern "C" {
bool rsf_rdr2_social_club_install(const SocialClubPlatform*, bool trace) noexcept;
void rsf_rdr2_social_club_dispatch() noexcept;
void rsf_rdr2_social_club_trace(const void* renderer, const void* list, uint32_t index) noexcept;
}

namespace {
using Dispatch = uintptr_t (*)(void*);
using Render = uintptr_t (*)(unsigned char*, void*, void*, void*, unsigned);
Dispatch original_dispatch{};
Render original_render{};

uintptr_t dispatch(void* context)
{
    rsf_rdr2_social_club_dispatch();
    return original_dispatch(context);
}

uintptr_t trace_render(unsigned char* renderer, void* draws, void* unused, void* list, unsigned index)
{
    rsf_rdr2_social_club_trace(renderer, list, index);
    return original_render(renderer, draws, unused, list, index);
}

bool current_index(void* swapchain, uint32_t* index) noexcept
{
#if defined(_MSC_VER)
    __try {
#endif
        *index = static_cast<IDXGISwapChain3*>(swapchain)->GetCurrentBackBufferIndex();
        return true;
#if defined(_MSC_VER)
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}

constexpr SocialClubPlatform platform{
    [](uintptr_t target) noexcept -> int32_t {
        return MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&dispatch),
            reinterpret_cast<void**>(&original_dispatch));
    },
    [](uintptr_t target) noexcept -> int32_t {
        return MH_CreateHook(reinterpret_cast<void*>(target), reinterpret_cast<void*>(&trace_render),
            reinterpret_cast<void**>(&original_render));
    },
    [](uintptr_t target) noexcept -> int32_t { return MH_EnableHook(reinterpret_cast<void*>(target)); },
    current_index,
};
}

bool rsf_rdr2_install_social_club_fix(bool trace) noexcept
{
    return rsf_rdr2_social_club_install(&platform, trace);
}
