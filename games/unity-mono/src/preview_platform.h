// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Unity Editor preview's native leaves
// (integrations/unity/native/preview.cpp) and their Rust owner (preview.rs). Not a DLL ABI and
// without generated bindings: preview.rs mirrors these records with size assertions.
//
// Rust owns the preview window record, the region admission, the D3D12 device, queue and fence
// references, the DXGI swapchain, each ring slot's command allocator, command list and source
// texture reference, the submission ring and the checked release. The leaves make the typed
// Win32, D3D12 and DXGI calls and never keep a reference past their return. Every transferred
// pointer is one reference that Rust drops through release.
#include <stdbool.h>
#include <stdint.h>
#include <rescaleframe/game_renderer.h>

struct rsf_unity_preview_rect {
    int32_t left, top, right, bottom;
};
struct rsf_unity_preview_texture {
    uint64_t width;
    uint32_t height, format;
};
// Transferred ID3D12Device, ID3D12CommandQueue, ID3D12Fence and IDXGISwapChain3 references.
struct rsf_unity_preview_gpu {
    void *device, *queue, *fence, *chain;
};
// One ring slot's transferred ID3D12CommandAllocator, ID3D12GraphicsCommandList (closed between
// submissions) and the ID3D12Resource its last submission copied from.
struct rsf_unity_preview_slot {
    void *allocator, *list, *source;
};

// record results.
#define RSF_UNITY_PREVIEW_RECORD_REFUSED 0u // A reset failed. The slot keeps its objects.
#define RSF_UNITY_PREVIEW_RECORD_BROKEN 1u  // Close failed. The slot's objects must be dropped.
#define RSF_UNITY_PREVIEW_RECORD_CLOSED 2u  // The copy is recorded and closed, not executed.

typedef void (*rsf_unity_preview_candidate)(void* user, void* window, const rsf_unity_preview_rect*);
// bind_gpu adds one reference to device and queue and creates the fence, transferring all three or
// nothing. create_chain, acquire and create_slot transfer each reference as soon as they hold it,
// partial results included. chain_extent, resize_chain, completed, wait, record, execute, signal
// and present borrow their arguments. completed does not wait; wait blocks until fence reaches
// value, 0 passing at once, and refuses after its timeout. retain adds one reference and release
// drops one, both through IUnknown; release accepts null.
struct rsf_unity_preview_leaves {
    void (*enumerate)(void* user, rsf_unity_preview_candidate);
    void* (*create_window)(void* parent, int32_t width, int32_t height);
    bool (*position)(void* window, void* parent, int32_t left, int32_t top, int32_t width, int32_t height);
    void (*show)(void* window, bool visible, bool asynchronous);
    bool (*bind_gpu)(void* device, void* queue, rsf_unity_preview_gpu* output);
    bool (*create_chain)(const rsf_unity_preview_gpu*, void* window, uint32_t width, uint32_t height, void** output);
    bool (*chain_extent)(void* chain, uint32_t* width, uint32_t* height);
    bool (*resize_chain)(void* chain, uint32_t width, uint32_t height);
    void (*describe)(void* texture, rsf_unity_preview_texture*);
    uint64_t (*completed)(void* fence);
    bool (*wait)(void* fence, uint64_t value);
    bool (*acquire)(void* chain, void** backbuffer);
    bool (*create_slot)(void* device, rsf_unity_preview_slot* output);
    uint32_t (*record)(const rsf_unity_preview_slot*, void* backbuffer, void* texture, bool common_state);
    void (*execute)(void* queue, void* list);
    bool (*signal)(void* queue, void* fence, uint64_t value);
    bool (*present)(void* chain, const rsf_game_render_pass*);
    void (*retain)(void* object);
    void (*release)(void* object);
};
static_assert(sizeof(rsf_unity_preview_rect) == 16 && sizeof(rsf_unity_preview_texture) == 16);
static_assert(sizeof(rsf_unity_preview_gpu) == 32 && sizeof(rsf_unity_preview_slot) == 24);
static_assert(sizeof(rsf_unity_preview_leaves) == 152);

extern "C" {
bool rsf_core_unity_preview_region(int32_t left, int32_t top, int32_t width, int32_t height, bool show,
    const rsf_unity_preview_leaves*);
bool rsf_core_unity_preview_enabled();
bool rsf_core_unity_preview_owns(void* window);
void* rsf_core_unity_preview_parent();
void rsf_core_unity_preview_hide(const rsf_unity_preview_leaves*);
uint64_t rsf_core_unity_preview_base_frames();
// Package retirement closes every bridge route to the preview first. Reads the fence without
// waiting: true records the proof that every submission completed and nothing faulted.
bool rsf_core_unity_preview_drained(const rsf_unity_preview_leaves*);
// Destroys every slot's allocator, list and source, then the swapchain, fence, queue and device,
// and forgets the ring. Refuses, destroying nothing, without a drained proof taken since the last
// submission.
bool rsf_core_unity_preview_release(const rsf_unity_preview_leaves*);
bool rsf_core_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height,
    const rsf_unity_preview_leaves*);
bool rsf_core_unity_preview_present(void* texture, const rsf_game_render_pass*, bool common_state,
    const rsf_unity_preview_leaves*);
}
