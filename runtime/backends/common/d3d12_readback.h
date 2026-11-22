// SPDX-License-Identifier: MIT
#pragma once
#include "d3d12_helpers.h"
#include "d3d12_readback_layout.h"
#include <wrl/client.h>

namespace rsf {
inline void* allocate_readback12(void* raw, rsf_fg12_readback_layout* out, bool single_slice) noexcept {
    auto* source = static_cast<ID3D12Resource*>(raw);
    const auto desc = source->GetDesc();
    if (desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D || desc.SampleDesc.Count != 1 || (single_slice && desc.DepthOrArraySize != 1)) return nullptr;
    Microsoft::WRL::ComPtr<ID3D12Device> device;
    if (FAILED(source->GetDevice(IID_PPV_ARGS(&device)))) return nullptr;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    UINT rows = 0; UINT64 row_bytes = 0, bytes = 0;
    device->GetCopyableFootprints(&desc, 0, 1, 0, &footprint, &rows, &row_bytes, &bytes);
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_READBACK;
    D3D12_RESOURCE_DESC storage{}; storage.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    storage.Width = bytes; storage.Height = storage.DepthOrArraySize = storage.MipLevels = 1;
    storage.SampleDesc.Count = 1; storage.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    ID3D12Resource* buffer = nullptr;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &storage, D3D12_RESOURCE_STATE_COPY_DEST,
        nullptr, IID_PPV_ARGS(&buffer)))) return nullptr;
    *out = {bytes, footprint.Offset, footprint.Footprint.Width, footprint.Footprint.Height,
        uint32_t(footprint.Footprint.Format), footprint.Footprint.RowPitch};
    return buffer;
}
inline void copy_readback12(void* list, void* raw, uint32_t state, void* buffer, const rsf_fg12_readback_layout* layout) noexcept {
    auto* commands = static_cast<ID3D12GraphicsCommandList*>(list);
    auto* source = static_cast<ID3D12Resource*>(raw);
    const auto before = static_cast<D3D12_RESOURCE_STATES>(state);
    transition(commands, source, before, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{}; from.pResource = source; from.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
    D3D12_TEXTURE_COPY_LOCATION to{}; to.pResource = static_cast<ID3D12Resource*>(buffer); to.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
    to.PlacedFootprint = {layout->offset, {static_cast<DXGI_FORMAT>(layout->format), layout->width, layout->height, 1, layout->row_pitch}};
    commands->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    transition(commands, source, D3D12_RESOURCE_STATE_COPY_SOURCE, before);
}
inline const uint8_t* map_readback12(void* buffer, uint64_t bytes) noexcept {
    void* data = nullptr;
    const D3D12_RANGE range{0, static_cast<size_t>(bytes)};
    return SUCCEEDED(static_cast<ID3D12Resource*>(buffer)->Map(0, &range, &data)) ? static_cast<const uint8_t*>(data) : nullptr;
}
inline void unmap_readback12(void* buffer) noexcept {
    const D3D12_RANGE written{0, 0};
    static_cast<ID3D12Resource*>(buffer)->Unmap(0, &written);
}
}
