// SPDX-License-Identifier: MIT
#pragma once
#include <windows.h>
#include <d3d12.h>
#include <cstdint>

extern "C" void rsf_core_d3d12_transition(void*, void*, uint32_t, uint32_t);
extern "C" void rsf_core_d3d12_copy_transitioned(void*, void*, uint32_t, uint32_t, void*, uint32_t, uint32_t);
extern "C" HRESULT rsf_core_d3d12_wait_native_fence(void*, uint64_t, void*, uint32_t);
extern "C" int32_t rsf_core_d3d12_create_command_list(void*, uint32_t, void**, void**);
extern "C" void* rsf_core_d3d12_create_uav_texture(void*, uint32_t, uint32_t, uint32_t);

namespace rsf {
inline void transition(ID3D12GraphicsCommandList* list, ID3D12Resource* resource,
                       D3D12_RESOURCE_STATES before, D3D12_RESOURCE_STATES after)
{
    rsf_core_d3d12_transition(list, resource, before, after);
}
inline void copy_transitioned(ID3D12GraphicsCommandList* list,
                              ID3D12Resource* destination, D3D12_RESOURCE_STATES destination_before,
                              D3D12_RESOURCE_STATES destination_after,
                              ID3D12Resource* source, D3D12_RESOURCE_STATES source_before,
                              D3D12_RESOURCE_STATES source_after)
{
    rsf_core_d3d12_copy_transitioned(list, destination, destination_before, destination_after,
        source, source_before, source_after);
}
// The caller owns the auto-reset event. The Rust wait retains the exact completion value and HRESULT.
inline HRESULT wait_fence(ID3D12Fence* fence, uint64_t value, HANDLE event, DWORD timeout_ms = 10000)
{
    return rsf_core_d3d12_wait_native_fence(fence, value, event, timeout_ms);
}
inline bool create_command_list(ID3D12Device* device, D3D12_COMMAND_LIST_TYPE type,
                                ID3D12CommandAllocator** allocator, ID3D12GraphicsCommandList** list)
{
    void* native_allocator = nullptr;
    void* native_list = nullptr;
    const bool created = rsf_core_d3d12_create_command_list(device, type, &native_allocator, &native_list) != 0;
    *allocator = static_cast<ID3D12CommandAllocator*>(native_allocator);
    *list = static_cast<ID3D12GraphicsCommandList*>(native_list);
    return created;
}
inline bool create_uav_texture(ID3D12Device* device, uint32_t width, uint32_t height, DXGI_FORMAT format,
                               ID3D12Resource** out)
{
    *out = static_cast<ID3D12Resource*>(rsf_core_d3d12_create_uav_texture(device, width, height, format));
    return *out != nullptr;
}
}
