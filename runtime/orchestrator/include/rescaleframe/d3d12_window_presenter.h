/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/game_renderer.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_D3D12_WINDOW_TARGET_ABI_VERSION 1u
typedef struct rsf_d3d12_window_presenter rsf_d3d12_window_presenter;
typedef struct rsf_d3d12_window_target {
    uint32_t struct_size;
    uint32_t abi_version;
    void* window;
    /* Nonzero, increasing on each new window lifetime, including HWND reuse. */
    uint64_t generation;
} rsf_d3d12_window_target;

/* Optional Runtime window observer consumes the prepared swapchain lease before Present.
   Returning zero refuses presentation. */
typedef int (*rsf_d3d12_window_observer)(const rsf_game_render_pass*);
rsf_d3d12_window_presenter* rsf_d3d12_window_create(rsf_d3d12_window_observer);
/* Device and direct queue are borrowed on entry. The presenter retains its own references. */
int rsf_d3d12_window_prepare(rsf_d3d12_window_presenter*, const rsf_d3d12_window_target*,
    void* device, void* queue, uint32_t width, uint32_t height);
/* The pass and source are leased through the call. The presenter retains the copied resource
   until completion. common_state selects COMMON instead of RENDER_TARGET at handoff. */
int rsf_d3d12_window_present(rsf_d3d12_window_presenter*, const rsf_d3d12_window_target*,
    void* source, const rsf_game_render_pass*, uint32_t common_state);
/* Close caller admission before checking and destroying. A failed proof retains all resources.
   The caller keeps the handle after a refusal. Any subsequent execution invalidates the proof. */
int rsf_d3d12_window_drained(rsf_d3d12_window_presenter*);
int rsf_d3d12_window_destroy(rsf_d3d12_window_presenter*);

#ifdef __cplusplus
}
#endif
