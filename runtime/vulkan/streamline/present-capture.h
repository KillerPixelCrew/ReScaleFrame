// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>

#define RSF_SL_PRESENT_CAPTURE_ABI_VERSION 1u
typedef struct rsf_sl_present_capture_packet {
    uint32_t struct_size, abi_version;
    void* device;
    void* queue;
    const void* present_info; /* VkPresentInfoKHR, borrowed only during callback */
    const void* swapchain_info; /* VkSwapchainCreateInfoKHR, borrowed only */
    uintptr_t native_present; /* PFN_vkQueuePresentKHR, bypasses SDK proxy */
    uint64_t source_frame;
    uint32_t present_frame;
    int32_t generated_index; /* SDK FrameBoundaryGuard ordinal; -1 is real */
    uint32_t boundary_valid;
    uint32_t queue_family;
    uintptr_t native_images; /* PFN_vkGetSwapchainImagesKHR, bypasses SDK proxy */
    uintptr_t native_submit; /* PFN_vkQueueSubmit, bypasses SDK proxy */
    uintptr_t native_wsi_call; /* LL2 mutex lease, no session/PCL reentry */
} rsf_sl_present_capture_packet;
typedef int32_t (*rsf_sl_present_capture_callback)(void* user, const rsf_sl_present_capture_packet*);
typedef struct rsf_sl_present_capture_registration {
    uint32_t struct_size, abi_version;
    void* device;
    rsf_sl_present_capture_callback callback;
    void* user;
} rsf_sl_present_capture_registration;
/* Private corrected-common export rsfSlSetPresentCapture. Registration is one
   device/owner lease. A callback replaces one REAL SDK WSI present and must call
   native_present exactly once after consuming its original wait semaphores.
   GPU submission/wait failure must be returned without attempting a second
   consumption of those binary waits. It may never call the shared SL session or PCL. Callback pointers and packet
   metadata are borrowed. Register/clear only after presenter and GPU drain.
   Clearing uses the same device/user with a null callback. This is diagnostic
   only; ordinal is recorded without guessing its target interpolation fraction. */
typedef int32_t (*rsf_sl_set_present_capture)(const rsf_sl_present_capture_registration*);
