/* SPDX-License-Identifier: MIT */
#define COBJMACROS
#include "asi_platform.h"
#include <windows.h>
#include <dxgi1_4.h>
#include <MinHook.h>
#include <rescaleframe/foreign_guard.h>

_Static_assert(MH_ERROR_ENABLED == 5 && MH_ERROR_MEMORY_PROTECT == 10, "MinHook status values");

/* The diagnostic pointer is borrowed from Social Club's private renderer.
   Every fault in the DXGI call and its implementation reads as unavailable. */
bool rsf_rdr2_social_club_current_index(void* swapchain, uint32_t* index)
{
    bool read = true;
    RSF_FOREIGN_TRY { *index = IDXGISwapChain3_GetCurrentBackBufferIndex((IDXGISwapChain3*)swapchain); }
    RSF_FAULT_CATCH { read = false; }
    return read;
}
