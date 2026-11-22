/* SPDX-License-Identifier: MIT */
#define COBJMACROS
#include "asi_platform.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <MinHook.h>

_Static_assert(MH_ERROR_ENABLED == 5 && MH_ERROR_MEMORY_PROTECT == 10, "MinHook status values");

/* The diagnostic pointer is borrowed from Social Club's private renderer.
   Guard the DXGI call and its implementation with Windows structured exception handling. */
bool rsf_rdr2_social_club_current_index(void* swapchain, uint32_t* index)
{
#if defined(_MSC_VER)
    __try {
#endif
        *index = IDXGISwapChain3_GetCurrentBackBufferIndex((IDXGISwapChain3*)swapchain);
        return true;
#if defined(_MSC_VER)
    } __except (EXCEPTION_EXECUTE_HANDLER) { return false; }
#endif
}
