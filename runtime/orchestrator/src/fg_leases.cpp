// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_leases.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <new>
using Microsoft::WRL::ComPtr;
// Each ring slot retains COM inputs through both application-queue and asynchronous vendor reads.
// No lock: the graphics owner serializes every transition acquire -> submit -> seal -> collect.
struct rsf_fg_leases {
    struct Slot {
        uint64_t id = 0;
        uint32_t generation = 0;
        ComPtr<ID3D12Resource> resources[5];
        ComPtr<ID3D12Fence> gpu, vendor;
        uint64_t gpu_value = 0, vendor_value = 0;
        bool submitted = false, sealed = false;
    } slots[16];
    uint32_t capacity = 0;
    uint64_t last_acquire = 0;
};
namespace {
rsf_fg_leases::Slot* find(rsf_fg_leases* leases, uint64_t id)
{
    if (!leases || !id) return nullptr;
    auto& slot = leases->slots[id % leases->capacity];
    return slot.id == id ? &slot : nullptr;
}
// UINT64_MAX from GetCompletedValue means device removal, never successful retirement.
rsf_backend_result ready(ID3D12Fence* fence, uint64_t value)
{
    if (!fence) return value ? RSF_BACKEND_ERROR_INVALID_ARGUMENT : RSF_BACKEND_OK;
    const auto completed = fence->GetCompletedValue();
    if (completed == UINT64_MAX) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    return completed >= value ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_NOT_READY;
}
bool same_device(ID3D12DeviceChild* first, ID3D12DeviceChild* second)
{
    ComPtr<ID3D12Device> a, b;
    return SUCCEEDED(first->GetDevice(IID_PPV_ARGS(&a))) &&
        SUCCEEDED(second->GetDevice(IID_PPV_ARGS(&b))) && a.Get() == b.Get();
}
}
extern "C" rsf_backend_result rsf_fg_leases_create(uint32_t capacity, rsf_fg_leases** out)
{
    if (!out || !capacity || capacity > 16) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *out = new (std::nothrow) rsf_fg_leases;
    if (!*out) return RSF_BACKEND_ERROR_INIT_FAILED;
    (*out)->capacity = capacity; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_leases_acquire(rsf_fg_leases* leases, uint64_t id,
    uint32_t generation, void* const* resources, uint32_t count)
{
    if (!leases || !id || !generation || !resources || !count || count > 5) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (id <= leases->last_acquire) return RSF_BACKEND_ERROR_STALE_RESOURCES;
    auto& slot = leases->slots[id % leases->capacity];
    if (slot.id) return RSF_BACKEND_ERROR_NOT_READY;
    for (uint32_t i = 0; i < count; ++i) if (!resources[i]) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    for (uint32_t i = 1; i < count; ++i)
        if (!same_device(static_cast<ID3D12Resource*>(resources[0]), static_cast<ID3D12Resource*>(resources[i])))
            return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    for (uint32_t i = 0; i < count; ++i) slot.resources[i] = static_cast<ID3D12Resource*>(resources[i]);
    slot.id = id; slot.generation = generation; leases->last_acquire = id; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_leases_submit(rsf_fg_leases* leases, uint64_t id, void* fence, uint64_t value)
{
    auto* slot = find(leases, id);
    if (!slot || !fence || !value || value == UINT64_MAX) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (slot->submitted) return RSF_BACKEND_ERROR_NOT_READY;
    if (!same_device(slot->resources[0].Get(), static_cast<ID3D12Fence*>(fence))) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    slot->gpu = static_cast<ID3D12Fence*>(fence); slot->gpu_value = value; slot->submitted = true;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_leases_seal(rsf_fg_leases* leases, uint64_t id, const rsf_fg_retirement* retirement)
{
    auto* slot = find(leases, id);
    if (!slot || !retirement || retirement->struct_size < sizeof(*retirement) ||
        (!retirement->fence && retirement->value) || retirement->value == UINT64_MAX)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (!slot->submitted || slot->sealed) return RSF_BACKEND_ERROR_NOT_READY;
    if (retirement->fence && !same_device(slot->resources[0].Get(), static_cast<ID3D12Fence*>(retirement->fence)))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    slot->vendor = static_cast<ID3D12Fence*>(retirement->fence); slot->vendor_value = retirement->value; slot->sealed = true;
    return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_leases_collect(rsf_fg_leases* leases, uint32_t* pending)
{
    if (!leases || !pending) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *pending = 0;
    rsf_backend_result result = RSF_BACKEND_OK;
    for (uint32_t i = 0; i < leases->capacity; ++i) {
        auto& slot = leases->slots[i]; if (!slot.id) continue;
        if (!slot.submitted || !slot.sealed) { ++*pending; continue; }
        const auto gpu = ready(slot.gpu.Get(), slot.gpu_value), vendor = ready(slot.vendor.Get(), slot.vendor_value);
        if (gpu == RSF_BACKEND_OK && vendor == RSF_BACKEND_OK) slot = {};
        else {
            ++*pending;
            if (gpu == RSF_BACKEND_ERROR_FEATURE_FAILED || vendor == RSF_BACKEND_ERROR_FEATURE_FAILED)
                result = RSF_BACKEND_ERROR_FEATURE_FAILED;
        }
    }
    return result;
}
extern "C" rsf_backend_result rsf_fg_leases_cancel(rsf_fg_leases* leases, uint64_t id)
{
    auto* slot = find(leases, id); if (!slot) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (slot->submitted) return RSF_BACKEND_ERROR_NOT_READY;
    *slot = {}; return RSF_BACKEND_OK;
}
extern "C" rsf_backend_result rsf_fg_leases_destroy(rsf_fg_leases* leases)
{
    if (!leases) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    uint32_t pending = 0;
    const auto result = rsf_fg_leases_collect(leases, &pending); if (result != 0) return result;
    if (pending) return RSF_BACKEND_ERROR_NOT_READY;
    delete leases; return RSF_BACKEND_OK;
}
