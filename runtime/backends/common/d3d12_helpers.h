// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <windows.h>
#include <d3d12.h>
#include <cstdint>

namespace rsf {
// Whole-resource transition. A no-op when the states already match, which D3D12 would otherwise
// reject as a redundant barrier on some drivers (fsr_generation was the only copy that checked).
inline void transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
                       D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    if (before == after) return;
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, before, after};
    list->ResourceBarrier(1, &barrier);
}
// CopyResource between two resources that sit in their own states: both go to the copy states,
// then back to the states they were in, destination first.
inline void copy_transitioned(ID3D12GraphicsCommandList* list,
                              ID3D12Resource* destination, D3D12_RESOURCE_STATES destination_before,
                              D3D12_RESOURCE_STATES destination_after,
                              ID3D12Resource* source, D3D12_RESOURCE_STATES source_before,
                              D3D12_RESOURCE_STATES source_after)
{
    transition(list, source, source_before, D3D12_RESOURCE_STATE_COPY_SOURCE);
    transition(list, destination, destination_before, D3D12_RESOURCE_STATE_COPY_DEST);
    list->CopyResource(destination, source);
    transition(list, destination, D3D12_RESOURCE_STATE_COPY_DEST, destination_after);
    transition(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_after);
}
// Wait for `value` on a fence, 10 seconds at most. A zero value is "nothing to wait for". The
// result separates the three ways it can fail: no fence (E_FAIL), a removed device
// (DXGI_ERROR_DEVICE_REMOVED, which the fence reports as UINT64_MAX) and a stuck queue
// (DXGI_ERROR_DEVICE_HUNG). `event` is an auto-reset event the caller owns.
inline HRESULT wait_fence(ID3D12Fence* fence, uint64_t value, HANDLE event, DWORD timeout_ms = 10000)
{
    if (!value) return S_OK;
    if (!fence || !event) return E_FAIL;
    const uint64_t done = fence->GetCompletedValue();
    if (done == UINT64_MAX) return DXGI_ERROR_DEVICE_REMOVED;
    if (done >= value) return S_OK;
    if (FAILED(fence->SetEventOnCompletion(value, event))) return E_FAIL;
    return WaitForSingleObject(event, timeout_ms) == WAIT_OBJECT_0 ? S_OK : DXGI_ERROR_DEVICE_HUNG;
}
// Allocator and list of one type, the list created in the closed state. On failure both are
// released and left null.
inline bool create_command_list(ID3D12Device* device, D3D12_COMMAND_LIST_TYPE type,
                                ID3D12CommandAllocator** allocator, ID3D12GraphicsCommandList** list)
{
    if (SUCCEEDED(device->CreateCommandAllocator(type, IID_PPV_ARGS(allocator))) &&
        SUCCEEDED(device->CreateCommandList(0, type, *allocator, nullptr, IID_PPV_ARGS(list))) &&
        SUCCEEDED((*list)->Close())) return true;
    if (*list) { (*list)->Release(); *list = nullptr; }
    if (*allocator) { (*allocator)->Release(); *allocator = nullptr; }
    return false;
}
// Default-heap 2D texture with UAV access, created in UNORDERED_ACCESS.
inline bool create_uav_texture(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format,
                               ID3D12Resource** out)
{
    D3D12_HEAP_PROPERTIES heap{}; heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    D3D12_RESOURCE_DESC desc{}; desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width; desc.Height = height; desc.DepthOrArraySize = 1; desc.MipLevels = 1;
    desc.Format = format; desc.SampleDesc.Count = 1; desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
    return SUCCEEDED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
        D3D12_RESOURCE_STATE_UNORDERED_ACCESS, nullptr, IID_PPV_ARGS(out)));
}
}
