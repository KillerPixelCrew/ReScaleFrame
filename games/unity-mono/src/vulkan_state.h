/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's Vulkan bridge (integrations/unity/native/
   vulkan_bridge.cpp) and its Rust state (vulkan_state.rs). Not a DLL ABI; its only structure is
   the packet bridge_abi.rs already binds, so it has no generated bindings of its own.

   Rust owns the shared device's readiness and its single creator, the bridge's permanent fault,
   the render events queued to Unity and their packets, the present interceptions and surface
   updates in progress, the player window frame the present renders under, and the checked close
   and release. The command slot rotation and the shared D3D12 fence values are the package
   bridge's submission ring (bridge_interop.h); a process has one Unity renderer, so the D3D11 and
   Vulkan bridges never use it at once. vulkan_bridge.cpp keeps the Unity interfaces, the Vulkan,
   D3D12 and DXGI objects and calls, images, command buffers, slot fences, waits, copies, barriers,
   the swapchain interception and the window.

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
/* Stores the player window frame identity the present interception renders under. Zero when the
   bridge faulted or is closing. */
uint32_t rsf_unity_vulkan_set_window(const rsf_unity_packet* packet);
/* Copies the stored window frame to packet. Zero without a frame or after a fault. */
uint32_t rsf_unity_vulkan_window(rsf_unity_packet* packet);

/* Copies packet into a free event slot of this kind. Returns the copy's address, which Unity hands
   back to the queue callback as identity only, and its slot for the caller's images; NULL when
   faulted, closing, of an unknown kind or full. */
void* rsf_unity_vulkan_admit_event(const rsf_unity_packet* packet, uint32_t kind, uint32_t reconfigure,
    uint32_t* slot);
/* Copies out the queued event at address and marks it running; it stays outstanding until
   rsf_unity_vulkan_finish_event. Zero when address names no queued event; a faulted bridge frees
   the event and returns zero. */
uint32_t rsf_unity_vulkan_start_event(const void* address, rsf_unity_packet* packet, uint32_t* kind,
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

/* Stops the bridge until release. Non-zero for the first fault, which the caller reports. */
uint32_t rsf_unity_vulkan_fault(void);
uint32_t rsf_unity_vulkan_faulted(void);

/* Non-zero when no event, entry or device creation is outstanding; *ready, when non-null,
   receives whether the shared device exists. */
uint32_t rsf_unity_vulkan_idle(uint32_t* ready);
/* Closes an idle bridge: events, entries and device claims are refused until release or reopen.
   Zero when work is outstanding or it is already closed. */
uint32_t rsf_unity_vulkan_close(void);
/* Reopens a closed bridge whose GPU work did not drain. */
void rsf_unity_vulkan_reopen(void);
/* Forgets the device readiness, fault, events and window frame, and opens the bridge. */
void rsf_unity_vulkan_release(void);
#ifdef __cplusplus
}
#endif
