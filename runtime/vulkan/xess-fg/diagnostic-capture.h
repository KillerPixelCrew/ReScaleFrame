// SPDX-License-Identifier: MIT
#pragma once
#include "capture_api.h"
#include "session_abi.h"
#include "../../common/include/rescaleframe/sha256_file.h"
#include <cstring>

/* Native leaves of the opt-in XeSS-FG diagnostic capture. The Rust owner in
   capture.rs decides configuration, source identity, admission, budgets,
   duplicates, the callback counter and completion-gated publication, and owns
   the pending readbacks. These leaves keep the typed SDK callback table, the
   DX12 readback, copy and map calls. */

static_assert(sizeof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT) == 32 &&
    offsetof(D3D12_PLACED_SUBRESOURCE_FOOTPRINT, Footprint) == 8 && offsetof(D3D12_SUBRESOURCE_FOOTPRINT, RowPitch) == 16);
static_assert(sizeof(rsf_frame_record) == 480 && alignof(rsf_frame_record) == 8);
static_assert(offsetof(rsf_frame_record, frame_id) == 8 && offsetof(rsf_frame_record, output_width) == 52 &&
    offsetof(rsf_frame_record, output_height) == 56 && offsetof(rsf_frame_record, input_qpc) == 64);
extern "C" {
// Checks the GameSDK record and camera ABI before binding an accepted source.
int32_t rsf_core_xess_capture_source(rsf_xess_capture_owner*, const rsf_frame_record*, uint32_t);
void rsf_core_xess_capture_input(rsf_xess_capture_owner*, uint64_t, uint32_t,
    const xefg_swapchain_d3d12_resource_data_t*, void*);
void rsf_core_xess_capture_output(rsf_xess_capture_owner*, uint64_t, uint32_t, void*, void*, uint32_t);
void rsf_core_xess_capture_before_outputs(rsf_xess_capture_owner*, uint64_t, uint32_t);
uint64_t rsf_core_xess_capture_callbacks(const rsf_xess_capture_owner*);
}

/* Private ABI for one pinned Intel binary, recovered from setter RVA20E1C0 and
   resource notifier RVAs20AB10/20AC40. This is not a public Intel SDK interface.
   Exact fingerprint admission precedes registration. Unknown callbacks stay0. */
namespace rsf_xess_capture {
struct DiagnosticsV2 {
    uint32_t version = 2, reserved = 0;
    void* user = nullptr;
    void* before_present = nullptr;
    void* before_commands = nullptr;
    void* after_commands = nullptr;
    void* constants = nullptr;
    void* before_inputs = nullptr;
    void (*input)(void*, uint64_t, uint32_t, const xefg_swapchain_d3d12_resource_data_t*, ID3D12GraphicsCommandList*) = nullptr;
    void* after_inputs = nullptr;
    void (*before_outputs)(void*, uint64_t, ID3D12GraphicsCommandList*, uint32_t) = nullptr;
    void (*output)(void*, uint64_t, uint32_t, ID3D12Resource*, ID3D12GraphicsCommandList*, uint32_t) = nullptr;
    void* after_outputs = nullptr;
};
static_assert(sizeof(DiagnosticsV2) == 0x60 && offsetof(DiagnosticsV2, input) == 0x38 && offsetof(DiagnosticsV2, output) == 0x50);
static_assert(sizeof(xefg_swapchain_d3d12_resource_data_t) == 0x28);
using SetDiagnostics = int (*)(xefg_swapchain_handle_t, const DiagnosticsV2*);

// The SDK user pointer is the Rust capture owner.
inline void Input(void* user, uint64_t token, uint32_t kind,
    const xefg_swapchain_d3d12_resource_data_t* data, ID3D12GraphicsCommandList* commands) noexcept {
    rsf_core_xess_capture_input(static_cast<rsf_xess_capture_owner*>(user), token, kind, data, commands);
}

inline void Output(void* user, uint64_t token, uint32_t kind,
    ID3D12Resource* resource, ID3D12GraphicsCommandList* commands, uint32_t ordinal) noexcept {
    rsf_core_xess_capture_output(static_cast<rsf_xess_capture_owner*>(user), token, kind, resource, commands, ordinal);
}

inline void BeforeOutputs(void* user, uint64_t token, ID3D12GraphicsCommandList*, uint32_t count) noexcept {
    if (user) rsf_core_xess_capture_before_outputs(static_cast<rsf_xess_capture_owner*>(user), token, count);
}

inline uint32_t Digest(void* module, unsigned char* sha256) {
    wchar_t path[32768]{};
    DWORD size = GetModuleFileNameW(static_cast<HMODULE>(module), path, 32768);
    if (!size || size >= 32768) return 0;
    return rsf_common_sha256_file(path, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, sha256);
}

inline void Describe(void* resource, rsf_xess_capture_desc* output) {
    const D3D12_RESOURCE_DESC desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    output->width = desc.Width;
    output->height = desc.Height;
    output->dimension = static_cast<uint32_t>(desc.Dimension);
    output->format = static_cast<uint32_t>(desc.Format);
    output->depth_or_array_size = desc.DepthOrArraySize;
    output->mip_levels = desc.MipLevels;
    output->sample_count = desc.SampleDesc.Count;
}

// Returns a device reference for the readback allocation, or -1 without one.
inline int32_t QueryFootprint(void* resource, void* footprint, uint64_t* bytes, void** device) {
    auto* source = static_cast<ID3D12Resource*>(resource);
    ID3D12Device* owner = nullptr;
    if (FAILED(source->GetDevice(IID_PPV_ARGS(&owner)))) return -1;
    const D3D12_RESOURCE_DESC desc = source->GetDesc();
    UINT rows = 0; UINT64 row_bytes = 0;
    owner->GetCopyableFootprints(&desc, 0, 1, 0, static_cast<D3D12_PLACED_SUBRESOURCE_FOOTPRINT*>(footprint),
        &rows, &row_bytes, bytes);
    *device = owner;
    return 0;
}

inline void* CreateReadback(void* device, uint64_t bytes) {
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC buffer{}; buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = bytes; buffer.Height = buffer.DepthOrArraySize = buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1; buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* readback = nullptr;
    const HRESULT created = static_cast<ID3D12Device*>(device)->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE,
        &buffer, D3D12_RESOURCE_STATE_COPY_DEST, nullptr, IID_PPV_ARGS(&readback));
    if (SUCCEEDED(created)) return readback;
    if (readback) readback->Release();
    return nullptr;
}

inline void ReleaseObject(void* object) {
    if (object) static_cast<IUnknown*>(object)->Release();
}

inline void Transition(ID3D12GraphicsCommandList* commands, ID3D12Resource* resource,
    D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after) {
    if (before == after) return;
    D3D12_RESOURCE_BARRIER barrier{}; barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = resource; barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = before; barrier.Transition.StateAfter = after;
    commands->ResourceBarrier(1, &barrier);
}

// Records into the borrowed SDK command list and restores the callback state.
inline void RecordCopy(void* commands, void* resource, void* readback, const void* footprint, uint32_t state) {
    auto* list = static_cast<ID3D12GraphicsCommandList*>(commands);
    auto* source = static_cast<ID3D12Resource*>(resource);
    const auto incoming = static_cast<D3D12_RESOURCE_STATES>(state);
    Transition(list, source, incoming, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(readback);
    to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = *static_cast<const D3D12_PLACED_SUBRESOURCE_FOOTPRINT*>(footprint);
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    Transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, incoming);
}

// Called only after the bridge drained its DX12 queue and completed its fence.
inline int32_t MapReadback(void* readback, uint64_t bytes, const uint8_t** output) {
    void* pixels = nullptr;
    const D3D12_RANGE range{0, static_cast<SIZE_T>(bytes)};
    const HRESULT result = static_cast<ID3D12Resource*>(readback)->Map(0, &range, &pixels);
    if (FAILED(result)) return -1;
    *output = static_cast<const uint8_t*>(pixels);
    return 0;
}

inline void UnmapReadback(void* readback) {
    const D3D12_RANGE written{0, 0};
    static_cast<ID3D12Resource*>(readback)->Unmap(0, &written);
}
}
