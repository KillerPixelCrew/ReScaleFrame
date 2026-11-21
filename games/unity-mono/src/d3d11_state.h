/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's D3D11 bridge (integrations/unity/native/
   bridge.cpp) and its Rust graphics owner (d3d11_state.rs). Not a DLL ABI and without generated
   bindings: d3d11_state.rs mirrors these records with size assertions and bridge.cpp checks the
   same sizes.

   Rust owns one shared graphics record, its command slots and its shared surfaces, published and
   retired as one unit: Unity's D3D11 device and immediate context (as ID3D11Device5 and
   ID3D11DeviceContext4), the D3D12 device and queue the render pass records on, the bridge's
   completion fence and the shared D3D11/D3D12 fence, one D3D12 command allocator and list per
   submission ring slot (RSF_UNITY_FRAMES_SLOTS, bridge_state.h), and six rsf_shared_surface slots:
   colour, depth, motion, output, the HUD-less scene and the Editor preview source. Rust also owns
   the single creator, the stopped state after a failed creation, each surface's source key, its
   replacement, the surface lease, the checked close and the release. The ring's slot rotation,
   completion values and shared fence values stay in bridge_interop.h.

   The D3D11 route admits its packets through rsf_unity_d3d11_admit_packet: the shared packet store
   (bridge_state.h) owns each queued packet's texture references from admission until the render
   event that takes it ends (rsf_unity_frames_end_packet) or rsf_unity_frames_release cancels it.

   bridge.cpp keeps Unity interface access, the typed D3D11, D3D12 and DXGI calls (the shared
   format choice, the rsf_shared_surface calls and the texture AddRef and Release among them), the
   render-pass calls and the frame callbacks. Its render callbacks are the bridge's
   admission: a caller borrows the published record only while it holds an rsf_unity_frame_callback,
   and the package closes callback admission (rsf_unity_frames_try_close) before it closes this
   owner. Inside a callback, a surface view is borrowed only under the surface lease. */
#include "bridge_state.h"

/* rsf_unity_d3d11_initialize results. */
#define RSF_UNITY_D3D11_DEVICE_BUSY 0u
/* This creation failed and rolled back. The caller reports it once. */
#define RSF_UNITY_D3D11_DEVICE_FAILED 1u
#define RSF_UNITY_D3D11_DEVICE_READY 2u
/* An earlier creation failed. Refused without a report until release. */
#define RSF_UNITY_D3D11_DEVICE_STOPPED 3u

#ifdef __cplusplus
extern "C" {
#endif
/* Every pointer is one transferred reference: ID3D11Device5, ID3D11DeviceContext4, ID3D12Device,
   ID3D12CommandQueue, ID3D12Fence (completion) and rsf_shared_fence (sharing). A copy is a view
   borrowed under a frame callback, the creation claim or the checked close. */
typedef struct rsf_unity_d3d11_device_handles {
    void* device11;
    void* context11;
    void* device12;
    void* queue12;
    void* completion;
    void* sharing;
} rsf_unity_d3d11_device_handles;

/* One ring slot's transferred ID3D12CommandAllocator and ID3D12GraphicsCommandList. The list is
   created closed. */
typedef struct rsf_unity_d3d11_command_handles {
    void* allocator;
    void* list;
} rsf_unity_d3d11_command_handles;

/* Typed native leaves. create and create_slot transfer each reference as soon as they hold it,
   partial results included, so Rust can roll a refused creation back with destroy_slot and
   destroy, which accept null fields. completed reads the completion fence and shared_completed
   the shared fence's D3D11 side; neither waits. wait blocks until the completion fence reaches
   value, 0 passing at once, and refuses after its timeout. create_surface creates one
   rsf_shared_surface of format on both devices, logging through services, and transfers it, null
   on refusal; destroy_surface accepts null. */
typedef struct rsf_unity_d3d11_device_ops {
    bool (*create)(const rsf_game_host_services* services, rsf_unity_d3d11_device_handles* output);
    void (*destroy)(const rsf_unity_d3d11_device_handles* shared);
    bool (*create_slot)(const rsf_unity_d3d11_device_handles* shared, rsf_unity_d3d11_command_handles* output);
    void (*destroy_slot)(const rsf_unity_d3d11_command_handles* slot);
    uint64_t (*completed)(const rsf_unity_d3d11_device_handles* shared);
    uint64_t (*shared_completed)(const rsf_unity_d3d11_device_handles* shared);
    bool (*wait)(const rsf_unity_d3d11_device_handles* shared, uint64_t value);
    bool (*create_surface)(const rsf_unity_d3d11_device_handles* shared, const rsf_game_host_services* services,
        uint32_t width, uint32_t height, uint32_t format, void** output);
    void (*destroy_surface)(void* surface);
} rsf_unity_d3d11_device_ops;

/* The caller holds a frame callback. READY lends a view of the published record. Otherwise the
   first caller claims creation and runs the leaves outside the state lock: the device, every
   command slot, then one publication. A refusal destroys what was created before the claim ends,
   so nothing partial is ever visible. Concurrent or closed callers see BUSY. services is borrowed
   for the creation leaf. */
uint32_t rsf_unity_d3d11_initialize(const rsf_game_host_services* services,
    const rsf_unity_d3d11_device_ops* ops, rsf_unity_d3d11_device_handles* view);
/* Copies the published record. False when closed or unpublished. The caller holds a frame callback
   through every use, or takes its own COM references before leaving it. */
bool rsf_unity_d3d11_device_view(rsf_unity_d3d11_device_handles* view);
/* Copies ring slot index of the published record. The caller holds a frame callback through
   recording and submission. */
bool rsf_unity_d3d11_command_slot(uint32_t index, rsf_unity_d3d11_command_handles* view);

/* The caller holds a frame callback. Lends the surface table to that caller alone until
   rsf_unity_d3d11_surfaces_end. False when closed, unpublished or already lent. */
bool rsf_unity_d3d11_surfaces_begin(void);
void rsf_unity_d3d11_surfaces_end(void);
/* The caller holds the surface lease. Writes surface index (0 to 5) as an rsf_shared_surface view
   valid until the lease ends. A surface whose key (the source texture's width, height and DXGI
   format) matches is borrowed as it is. Otherwise Rust waits for the last completion value handed
   out when a surface is being replaced, creates one of shared_format, publishes it and only then
   destroys the old one. A refused wait or creation keeps the old surface. A slot this lease already
   borrowed is never replaced within it. services is borrowed for the creation leaf. */
bool rsf_unity_d3d11_prepare_surface(uint32_t index, const rsf_game_host_services* services,
    uint32_t width, uint32_t height, uint32_t source_format, uint32_t shared_format, void** view);

/* Serialized by the package lifecycle, after rsf_unity_frames_try_close succeeded. Closes this
   owner (no new view, claim, creation or surface lease) and polls, without waiting, the completion
   fence against the ring and the shared fence against the last D3D11 copy. Device removal
   (UINT64_MAX) refuses. Without a record both read as 0. Refuses without closing while a creation
   runs or the surface lease is held. A fence refusal keeps the owner closed for a retry. */
bool rsf_unity_d3d11_try_close(void);
/* After a successful close: destroys the surfaces, the command slots, then the shared fence,
   completion fence, queue and both devices, and forgets the submission ring. The owner stays closed
   until open. False, destroying nothing, when it was not closed or a creation is in progress. */
bool rsf_unity_d3d11_release(void);
/* Resumes admission for the next package session. */
void rsf_unity_d3d11_open(void);

/* Typed reference leaves for Unity's D3D11 textures, as IUnknown: retain adds one reference
   (AddRef) and release drops one (Release). Neither is called with null. Rust runs retain under
   the packet store's lock and release outside it. */
typedef struct rsf_unity_d3d11_texture_ops {
    void (*retain)(void* texture);
    void (*release)(void* texture);
} rsf_unity_d3d11_texture_ops;

/* Admits packet as rsf_unity_frames_admit_packet does and, before the stored address is returned,
   retains its non-null colour, depth, motion and output textures for the stored copy. A refused
   packet retains nothing and returns NULL. The render event that takes the packet keeps its slot
   until rsf_unity_frames_end_packet, which releases the textures; rsf_unity_frames_release cancels
   a packet still queued or consuming. Independent of the graphics record. */
void* rsf_unity_d3d11_admit_packet(const rsf_unity_packet* packet, uint64_t session_id,
    const rsf_unity_d3d11_texture_ops* ops);
#ifdef __cplusplus
}
#endif
