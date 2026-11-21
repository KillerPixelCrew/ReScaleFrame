/* SPDX-License-Identifier: MIT */
#pragma once
/* Internal boundary between the Unity package's D3D11 bridge (integrations/unity/native/
   bridge.cpp) and its Rust pass planning (d3d11_pass.rs). Not a DLL ABI and without generated
   bindings: d3d11_pass.rs asserts the plan's layout and bridge.cpp checks the same offsets and the
   D3D12 state, DXGI format and preview flag values these records carry.

   Rust chooses each render event's route from its offset in the bridge's reserved event range,
   assembles the host render pass from the taken packet, plans the shared surface slot and D3D12
   state of each input, gates a pending policy change on the completion of every submission and
   decides whether a submission whose queue work went out retires or stops the adapter. The shared
   surface format (rsf_unity_d3d11_prepare_surface, d3d11_state.h) is chosen there too.

   bridge.cpp keeps every typed D3D11, D3D12 and DXGI call in its order: the packet textures'
   QueryInterface, the swapchain, the surface copies, the fence signals and waits, the barriers
   (D3D12_RESOURCE_STATE_COMMON to the planned state and back), list execution, the output copy and
   the render-pass calls. It fills the pass's resources and swapchain. */
#include "bridge_state.h"

/* rsf_unity_d3d11_pass_plan::route. */
/* The window pass on the engine swapchain, issued on the D3D12 queue. */
#define RSF_UNITY_D3D11_ROUTE_WINDOW 1u
/* The Editor's window pass: the packet colour is copied into surface 5 and handed to the queue. */
#define RSF_UNITY_D3D11_ROUTE_PREVIEW 2u
/* Reconstruction: inputs copied into surfaces 0 to 3, the output copied back after hand-back. */
#define RSF_UNITY_D3D11_ROUTE_RECONSTRUCT 3u
/* The HUD-less scene for frame generation in surface 4, from the swapchain without a colour. */
#define RSF_UNITY_D3D11_ROUTE_HUDLESS 4u

/* rsf_unity_d3d11_retire results. Every result but RETIRED has stopped the adapter. */
#define RSF_UNITY_D3D11_RETIRED 0u
#define RSF_UNITY_D3D11_HANDBACK_FAILED 1u
#define RSF_UNITY_D3D11_SIGNAL_FAILED 2u

/* RSF_UNITY_PREVIEW_SOURCE_COMMON (integrations/unity/native/package.h), set on a preview pass. */
#define RSF_UNITY_D3D11_PREVIEW_SOURCE_COMMON 0x40000000u

#ifdef __cplusplus
extern "C" {
#endif
/* One render event's plan. surfaces and states list colour, depth, motion and output in packet
   order; states are D3D12_RESOURCE_STATES values. pass carries no resources and no swapchain. */
typedef struct rsf_unity_d3d11_pass_plan {
    uint32_t route;
    /* HUD-less without a packet colour: the colour is buffer 0 of Unity's swapchain. */
    uint32_t swapchain_color;
    /* Copy the output surface back into the packet's output after the hand-back. */
    uint32_t copy_back;
    /* rsf_unity_d3d11_apply_change may mark this pass. */
    uint32_t reconfigurable;
    uint32_t surfaces[4];
    uint32_t states[4];
    rsf_game_render_pass pass;
} rsf_unity_d3d11_pass_plan;

/* Plans event offset (render event id minus the first reserved id) for packet. editor is the
   package running in the Unity Editor. False, writing nothing, outside the reserved range. */
bool rsf_unity_d3d11_plan_pass(uint32_t offset, const rsf_unity_packet* packet, bool editor,
    rsf_unity_d3d11_pass_plan* plan);
/* The caller holds a frame callback with the published graphics record. On a reconfigurable plan
   with a pending policy change, waits through the record's wait leaf for the last completion value
   handed out (rsf_unity_interop_submitted), settles that change and marks the pass
   RSF_GAME_RENDER_RECONFIGURE with an invalid camera. False when that wait refused; the change
   stays pending. True when nothing applies. */
bool rsf_unity_d3d11_apply_change(rsf_unity_d3d11_pass_plan* plan);
/* A submission whose queue work went out. signalled is its completion signal on the bridge's
   fence, published even when handed_back (the shared fence hand-back to D3D11) refused. Any result
   but RSF_UNITY_D3D11_RETIRED has deactivated the bridge; the caller logs it. */
uint32_t rsf_unity_d3d11_retire(bool signalled, bool handed_back);
#ifdef __cplusplus
}
#endif
