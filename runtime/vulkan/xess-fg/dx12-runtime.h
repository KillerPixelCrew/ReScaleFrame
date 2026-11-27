/* SPDX-License-Identifier: MIT */
#pragma once

/* Typed DXGI and D3D12 calls owned by the Runtime crate
   (frame_capture12/native/xess/context.rs) for the integrated bridge. */
#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_xess_dx12_timestamps rsf_xess_dx12_timestamps;
int32_t rsf_core_xess_dx12_factory(void**);
int32_t rsf_core_xess_dx12_adapter(void*, uint64_t, void**);
int32_t rsf_core_xess_dx12_device(void*, void**);
int32_t rsf_core_xess_dx12_queue(void*, void**);
uint32_t rsf_core_xess_dx12_tearing(void*);
void rsf_core_xess_dx12_swapchain_desc(DXGI_SWAP_CHAIN_DESC1*, uint32_t, uint32_t, uint32_t);
int32_t rsf_core_xess_dx12_recorders(void*, void* (*)[RSF_XESS_BRIDGE_ALLOCATOR_COUNT], void**);
int32_t rsf_core_xess_dx12_reset(void*, void*);
int32_t rsf_core_xess_dx12_close(void*);
int32_t rsf_core_xess_dx12_backbuffer(void*, void**);
int32_t rsf_core_xess_dx12_present(void*, uint32_t, uint32_t);
void rsf_core_xess_dx12_execute(void*, void*);
int32_t rsf_core_xess_dx12_fence(void*, uint32_t, void**);
int32_t rsf_core_xess_dx12_vulkan_luid(void*, uint64_t*);
int32_t rsf_core_xess_dx12_share_fence(void*, void*, void**);
int32_t rsf_core_xess_dx12_completed(void*, uint64_t*);
int32_t rsf_core_xess_dx12_arm(void*, uint64_t, void*);
int32_t rsf_core_xess_dx12_wait(void*, void*, uint64_t);
int32_t rsf_core_xess_dx12_signal(void*, void*, uint64_t);
void rsf_core_xess_dx12_timestamps_destroy(rsf_xess_dx12_timestamps*);
uint32_t rsf_core_xess_dx12_timestamps_begin(rsf_xess_dx12_timestamps**, void*, void*,
    uint32_t, const rsf_xess_timing*, int64_t);
void rsf_core_xess_dx12_timestamps_copy(rsf_xess_dx12_timestamps*, void*, uint32_t, uint32_t);
void rsf_core_xess_dx12_timestamps_enter(rsf_xess_dx12_timestamps*, uint32_t, int64_t);
void rsf_core_xess_dx12_timestamps_finish(rsf_xess_dx12_timestamps*, void*, uint32_t, int64_t);
#ifdef __cplusplus
}
#endif
#endif
