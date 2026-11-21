/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the shared Unity D3D12 bridge (bridge.cpp) and its Rust frame-state
   owner (bridge_state.rs). Not a DLL ABI: the game plugin and the Unity package each link
   bridge.cpp with the rsf-unity-mono archive. tools/generate-rust-bindings.py generates
   bridge_abi.rs from this header.

   Rust owns packet admission and storage, the policy generation, frame admission into the
   command ring, source-submission bookkeeping and retirement. bridge.cpp keeps the Unity
   interfaces, D3D12 objects, resource leases, render-pass calls and CPU input events.
   Fence values are Unity frame-fence completions read by bridge.cpp; 0 means no fence.

   The Unity package's D3D11 and Vulkan bridge (integrations/unity/native/bridge.cpp) uses the
   same activity, managed stage, callbacks, packet store, policy and config. Its D3D11 route's
   packets carry their texture references in the store (d3d11_state.h). A process has one Unity
   renderer, so the two bridges never share this state at once. */
#include <rescaleframe/unity_bridge.h>

/* Packet storage and source-submission ring sizes. bridge.cpp sizes its lease arrays to match. */
#define RSF_UNITY_FRAMES_PACKETS 32u
#define RSF_UNITY_FRAMES_SLOTS 3u
#define RSF_UNITY_FRAMES_SOURCES 8u

/* A reconstruction frame admitted into command ring slot `slot`. reconfigure is non-zero when
   this frame applies a pending policy change. */
typedef struct rsf_unity_frame_admission {
    uint32_t slot, reconfigure;
} rsf_unity_frame_admission;

/* A recorded source frame whose engine submission is not yet confirmed to the host. */
typedef struct rsf_unity_source_submission {
    uint64_t session_id, source_frame_id, view_key;
} rsf_unity_source_submission;

#ifdef __cplusplus
extern "C" {
#endif
/* Producer state. Inactive bridges refuse packets, frames and CPU events. */
void rsf_unity_frames_activate(uint32_t enabled);
uint32_t rsf_unity_frames_active(void);
/* Managed helper stage, stored for the lifecycle while callback admission is open. */
void rsf_unity_frames_report_managed(uint32_t stage);
uint32_t rsf_unity_frames_managed_stage(void);

/* Validates the packet for session_id and copies it into a free slot. Returns the copy's address,
   which Unity hands back to the render event as identity only, and its slot for the caller's
   leases; NULL when inactive, invalid or full. */
void* rsf_unity_frames_admit_packet(const rsf_unity_packet* packet, uint64_t session_id, uint32_t* slot);
/* Copies out the queued packet stored at address and frees its slot. A packet admitted with a
   lease (rsf_unity_d3d11_admit_packet, d3d11_state.h) instead keeps its slot and references,
   consuming, until rsf_unity_frames_end_packet; its address cannot be taken again meanwhile. Zero
   when address names no queued packet. */
uint32_t rsf_unity_frames_take_packet(const void* address, rsf_unity_packet* packet, uint32_t* slot);
/* The render event that took a packet from slot has ended. A consuming slot is freed and its
   references released outside the store's lock; any other slot is left alone. */
void rsf_unity_frames_end_packet(uint32_t slot);

/* Fills output from the host render configuration and the current policy. Zero when output is
   not this ABI's structure or the host refuses. services is borrowed for the call. */
int rsf_unity_frames_config(const rsf_game_host_services* services, uint32_t output_width,
    uint32_t output_height, rsf_unity_config* output);
/* Validates and stores the engine policy, advancing the generation. Zero when out of range. */
int rsf_unity_frames_set_policy(uint32_t engine_spatial, uint32_t quality);

/* The pending policy change for bridges that apply it outside rsf_unity_frames_admit_frame (the
   package's D3D11 and Vulkan events). Non-zero with its generation in *generation while one is
   pending; config reports the bridge disabled until it settles. */
uint32_t rsf_unity_frames_pending_change(uint32_t* generation);
/* Clears the pending change when it is still this generation; a newer change stays pending.
   Non-zero when cleared. */
uint32_t rsf_unity_frames_settle_change(uint32_t generation);

/* Admits a reconstruction packet against the policy generation, extents and ring completion. */
uint32_t rsf_unity_frames_admit_frame(const rsf_unity_packet* packet, uint64_t completed,
    rsf_unity_frame_admission* admission);
/* Records the slot's Unity completion and its source. Zero or the device-removed sentinel stops
   the bridge with ownership unknown and returns zero. */
uint32_t rsf_unity_frames_submitted(const rsf_unity_frame_admission* admission, uint64_t complete,
    const rsf_unity_source_submission* source);
/* Records a source copied on Unity's own recording list, completing at inline_fence. */
void rsf_unity_frames_record_source(const rsf_unity_source_submission* source, uint64_t inline_fence);
/* Removes this session's sources up to source_frame_id into output, RSF_UNITY_FRAMES_SOURCES
   entries, in ring order. Returns the count. */
uint32_t rsf_unity_frames_take_flushed(uint64_t session_id, uint64_t source_frame_id,
    rsf_unity_source_submission* output);

/* Callback admission. Non-zero must be paired with leave. Queued render callbacks can still
   enter while inactive to return their packets. */
uint32_t rsf_unity_frames_callback_enter(void);
void rsf_unity_frames_callback_leave(void);
/* The package closes callback admission once inactive, without queued or consuming packets or
   callbacks.
   A closed bridge stays closed through release and reopens on activation of the next session. */
uint32_t rsf_unity_frames_try_close(void);
/* Non-zero when no callback, packet or submission is outstanding at this completion. */
uint32_t rsf_unity_frames_drained(uint64_t completed);
/* Forgets packets, ring completions, sources, extents and the managed stage, releasing the
   references of any leased packet still queued or consuming. The activity flag and the policy,
   including its generation, persist, as does a package retirement close. */
void rsf_unity_frames_release(void);
#ifdef __cplusplus
}
#endif
