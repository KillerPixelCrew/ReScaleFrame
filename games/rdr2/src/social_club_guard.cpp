// SPDX-License-Identifier: MIT
#include "asi_platform.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <MinHook.h>

static_assert(MH_ERROR_ENABLED == 5 && MH_ERROR_MEMORY_PROTECT == 10);

// The diagnostic pointer is borrowed from Social Club's private renderer. Keep the Windows
// structured exception guard around the DXGI call, including the wrapper's own implementation.
extern "C" bool rsf_rdr2_social_club_current_index(void* swapchain, uint32_t* index)
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
