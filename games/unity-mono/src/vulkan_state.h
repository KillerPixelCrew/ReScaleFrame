/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's Vulkan bridge (integrations/unity/native/
   vulkan_bridge.cpp) and its Rust state (vulkan_state.rs). Not a DLL ABI and without generated
   bindings: it passes the packet bridge_abi.rs already binds and rsf_unity_vulkan_present_source,
   which vulkan_state.rs mirrors with a size assertion and vulkan_bridge.cpp checks the same way.

   Rust owns the shared device's readiness and its single creator, the bridge's permanent fault,
   the render events queued to Unity and their packets, the present interceptions and surface
   updates in progress, the player window frame the present renders under, whether the DXGI swap
   chain exists, the checked close and release, and the presentation: Unity's intercepted
   swapchains with their images and present-ready semaphores, the presentation window's identity
   and the presentation lease. The command slot rotation and the shared D3D12 fence values are the
   package bridge's submission ring (bridge_interop.h); a process has one Unity renderer, so the
   D3D11 and Vulkan bridges never use it at once. vulkan_bridge.cpp keeps the Unity interfaces, the
   Vulkan, D3D12 and DXGI objects and calls, images, command buffers, slot fences, waits, copies,
   barriers, swapchain and semaphore creation and destruction, the window and its swap chain.

   Event slots number RSF_UNITY_FRAMES_PACKETS: each event carries one packet taken from the
   shared store. vulkan_bridge.cpp sizes its image arrays to match. */
#include "bridge_state.h"

/* Render event kinds, by offset from the bridge's first event ID. */
#define RSF_UNITY_VULKAN_EVENT_SR 0u
#define RSF_UNITY_VULKAN_EVENT_WINDOW 1u
#define RSF_UNITY_VULKAN_EVENT_HUDLESS 2u
/* rsf_unity_vulkan_claim_device results. */
#define RSF_UNITY_VULKAN_DEVICE_BUSY 0u
#define RSF_UNITY_VULKAN_DEVICE_CLAIMED 1u
#define RSF_UNITY_VULKAN_DEVICE_READY 2u

#ifdef __cplusplus
extern "C" {
#endif
/* The frame and its process-lived host callbacks are copied together at admission. */
typedef struct rsf_unity_vulkan_frame {
    rsf_unity_packet packet;
    rsf_game_host_services services;
} rsf_unity_vulkan_frame;
/* Stores the player window frame. Zero when the bridge faulted or is closing. */
uint32_t rsf_unity_vulkan_set_window(const rsf_unity_vulkan_frame* frame);
/* Copies the stored window frame and its services. Zero without a frame or after a fault. */
uint32_t rsf_unity_vulkan_window(rsf_unity_vulkan_frame* frame);

/* Copies frame into a free event slot of this kind. Returns the copy's address, which Unity hands
   back to the queue callback as identity only, and its slot for the caller's images; NULL when
   faulted, closing, of an unknown kind or full. */
void* rsf_unity_vulkan_admit_event(const rsf_unity_vulkan_frame* frame, uint32_t kind, uint32_t reconfigure,
    uint32_t* slot);
/* Copies out the queued event at address and marks it running; it stays outstanding until
   rsf_unity_vulkan_finish_event. Zero when address names no queued event; a faulted bridge frees
   the event and returns zero. */
uint32_t rsf_unity_vulkan_start_event(const void* address, rsf_unity_vulkan_frame* frame, uint32_t* kind,
    uint32_t* reconfigure, uint32_t* slot);
void rsf_unity_vulkan_finish_event(uint32_t slot);

/* A present interception or surface update in progress. Zero, and not counted, while closing. */
uint32_t rsf_unity_vulkan_enter(void);
void rsf_unity_vulkan_leave(void);

/* Claims shared device creation: READY when created, CLAIMED when the caller must create it and
   report with rsf_unity_vulkan_device_created, BUSY while another thread creates it or the bridge
   is closing. */
uint32_t rsf_unity_vulkan_claim_device(void);
/* Ends a claim. A failed creation leaves the device absent for a later claim. */
void rsf_unity_vulkan_device_created(uint32_t created);
uint32_t rsf_unity_vulkan_ready(void);

/* Shared-image plans and per-recording ownership. The native replace callback drains and builds
   a complete replacement before it commits the graphics handles. */
typedef bool (*rsf_unity_vulkan_replace_image)(uint32_t index, uint32_t width, uint32_t height,
    uint32_t format, uint32_t dx_format);
bool rsf_unity_vulkan_prepare_image(uint32_t index, uint32_t width, uint32_t height,
    uint32_t format, uint32_t dx_format, rsf_unity_vulkan_replace_image replace);
/* Returns whether the recorded transition can start in GENERAL. */
bool rsf_unity_vulkan_record_image(uint32_t index);
void rsf_unity_vulkan_begin_recording(void);
/* Called only after the native queue accepted the upload or the fenced hand-back. */
void rsf_unity_vulkan_upload_submitted(void);
void rsf_unity_vulkan_handback_fenced(void);

/* Stops the bridge until release. Non-zero for the first fault, which the caller reports. */
uint32_t rsf_unity_vulkan_fault(void);
uint32_t rsf_unity_vulkan_faulted(void);

/* Enters an idle bridge to inspect the native fences. Non-zero must be paired with leave.
   *ready, when non-null, receives whether the shared device exists. */
uint32_t rsf_unity_vulkan_probe_idle(uint32_t* ready);
/* Closes an idle bridge: events, entries and device claims are refused until release or reopen.
   Zero when work is outstanding or it is already closed. */
uint32_t rsf_unity_vulkan_close(void);
/* Reopens a closed bridge whose GPU work did not drain. */
void rsf_unity_vulkan_reopen(void);
/* Forgets the device readiness, fault, events, window frame and swap chain, and opens the bridge. */
void rsf_unity_vulkan_release(void);
/* The bridge's DXGI swap chain was created; release forgets it with the other objects. */
void rsf_unity_vulkan_surface_created(void);
uint32_t rsf_unity_vulkan_surface_ready(void);

/* Presentation: Unity's intercepted swapchains, the presentation window and the presentation lease
   outlive release. Vulkan handles are passed as uint64_t identity. The lease is exclusive: a
   present interception holds it from lookup through the forwarded vkQueuePresentKHR, swapchain
   retirement through vkDeviceWaitIdle and semaphore destruction, and the window thread while it
   creates the DXGI swap chain. */

/* Keeps the first presentation window. Non-zero when window is the kept one. */
uint32_t rsf_unity_vulkan_set_surface_window(void* window);
void* rsf_unity_vulkan_surface_window(void);

/* Publishes a complete swapchain with one present-ready semaphore per image, waiting for the lease
   to end. Zero for a null handle or array, no images or a handle already published; the caller then
   destroys what it created. */
uint32_t rsf_unity_vulkan_swapchain_register(uint64_t chain, uint32_t width, uint32_t height,
    const uint64_t* images, const uint64_t* present_ready, uint32_t count);
/* Waits for the lease, forgets chain and, holding the lease, hands retire its present-ready
   semaphores to destroy. Zero, without calling retire, for an unknown chain or a null retire. */
typedef void (*rsf_unity_vulkan_retire_fn)(void* context, const uint64_t* present_ready, uint32_t count);
uint32_t rsf_unity_vulkan_swapchain_retire(uint64_t chain, rsf_unity_vulkan_retire_fn retire, void* context);

typedef struct rsf_unity_vulkan_present_source {
    uint64_t image;         /* VkImage at the presented index */
    uint64_t present_ready; /* VkSemaphore the forwarded present waits on */
    uint32_t width;
    uint32_t height;
} rsf_unity_vulkan_present_source;
/* Waits for and takes the lease, which the caller always ends with
   rsf_unity_vulkan_presentation_end. Non-zero, with source filled, when chain is published and
   index names one of its images. */
uint32_t rsf_unity_vulkan_present_begin(uint64_t chain, uint32_t index, rsf_unity_vulkan_present_source* source);
/* Takes the lease without waiting for a surface update or queue callback. */
uint32_t rsf_unity_vulkan_presentation_try_begin(void);
void rsf_unity_vulkan_presentation_end(void);
#ifdef __cplusplus
}
#endif
