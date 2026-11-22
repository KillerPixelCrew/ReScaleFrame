/* SPDX-License-Identifier: MIT */
#pragma once
#include "capture_api.h"
#include "session_abi.h"

/* C declarations for the opt-in XeSS-FG diagnostic capture. The Rust owner in
   capture.rs decides configuration, source identity, admission, budgets,
   duplicates, the callback counter and completion-gated publication, and owns
   the pending readbacks. bridge.c keeps the typed SDK callback table and the
   DX12 readback, copy and map calls. Include after the XeSS-FG and D3D12 headers. */

_Static_assert(sizeof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT) == 32 &&
    offsetof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT, Footprint) == 8 &&
    offsetof(D3D12_SUBRESOURCE_FOOTPRINT, RowPitch) == 16, "D3D12 footprint layout");
_Static_assert(sizeof(rsf_frame_record) == 480 && _Alignof(rsf_frame_record) == 8, "GameSDK frame layout");
_Static_assert(offsetof(rsf_frame_record, frame_id) == 8 && offsetof(rsf_frame_record, output_width) == 52 &&
    offsetof(rsf_frame_record, output_height) == 56 && offsetof(rsf_frame_record, input_qpc) == 64,
    "GameSDK frame fields");

#ifdef __cplusplus
extern "C" {
#endif

/* Checks the GameSDK record and camera ABI before binding an accepted source. */
int32_t rsf_core_xess_capture_source(rsf_xess_capture_owner*, const rsf_frame_record*, uint32_t);
void rsf_core_xess_capture_input(rsf_xess_capture_owner*, uint64_t, uint32_t,
    const xefg_swapchain_d3d12_resource_data_t*, void*);
void rsf_core_xess_capture_output(rsf_xess_capture_owner*, uint64_t, uint32_t, void*, void*, uint32_t);
void rsf_core_xess_capture_before_outputs(rsf_xess_capture_owner*, uint64_t, uint32_t);
uint64_t rsf_core_xess_capture_callbacks(const rsf_xess_capture_owner*);

/* Private ABI for one pinned Intel binary, recovered from setter RVA 20E1C0 and
   resource notifier RVAs 20AB10/20AC40. This is not a public Intel SDK interface.
   Exact fingerprint admission precedes registration. Unknown callbacks stay 0. */
typedef struct rsf_xess_diagnostics_v2 {
    uint32_t version, reserved;
    void* user;
    void* before_present;
    void* before_commands;
    void* after_commands;
    void* constants;
    void* before_inputs;
    void (*input)(void*, uint64_t, uint32_t, const xefg_swapchain_d3d12_resource_data_t*, ID3D12GraphicsCommandList*);
    void* after_inputs;
    void (*before_outputs)(void*, uint64_t, ID3D12GraphicsCommandList*, uint32_t);
    void (*output)(void*, uint64_t, uint32_t, ID3D12Resource*, ID3D12GraphicsCommandList*, uint32_t);
    void* after_outputs;
} rsf_xess_diagnostics_v2;
_Static_assert(sizeof(rsf_xess_diagnostics_v2) == 0x60 && offsetof(rsf_xess_diagnostics_v2, input) == 0x38 &&
    offsetof(rsf_xess_diagnostics_v2, output) == 0x50, "diagnostic callback table");
_Static_assert(sizeof(xefg_swapchain_d3d12_resource_data_t) == 0x28, "SDK resource data");
#define RSF_XESS_DIAGNOSTICS_VERSION 2u
typedef int (*rsf_xess_set_diagnostics)(xefg_swapchain_handle_t, const rsf_xess_diagnostics_v2*);

#if defined(RSF_XESS_BRIDGE_RUNTIME_D3D12)
/* Typed DX12 calls owned by the Runtime crate (frame_capture12/native/xess). */
void rsf_core_xess_d3d12_capture_describe(void*, rsf_xess_capture_desc*);
int32_t rsf_core_xess_d3d12_capture_footprint(void*, void*, uint64_t*, void**);
void* rsf_core_xess_d3d12_capture_readback(void*, uint64_t);
void rsf_core_xess_d3d12_capture_release(void*);
void rsf_core_xess_d3d12_capture_copy(void*, void*, void*, const void*, uint32_t);
int32_t rsf_core_xess_d3d12_capture_map(void*, uint64_t, const uint8_t**);
void rsf_core_xess_d3d12_capture_unmap(void*);
void rsf_core_xess_d3d12_present_describe(void*, rsf_xess_bridge_resource_desc*);
void rsf_core_xess_d3d12_present_transition(void*, void*, uint32_t, uint32_t);
void rsf_core_xess_d3d12_present_copy(void*, void*, void*);
int32_t rsf_core_xess_d3d12_shared_image(void*, const rsf_xess_bridge_shared_image_desc*, void**, void**);
#endif

#ifdef __cplusplus
}
#endif
