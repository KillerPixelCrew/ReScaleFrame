// SPDX-License-Identifier: MIT
// Guarded SDK queries used by the typed Rust DXGI and D3D12 wrappers.
#include <d3d11.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <iterator>

#include "census_internal.h"
#include "d3d_platform.h"

static_assert(sizeof(D3D12_RESOURCE_DESC) == 56 && alignof(D3D12_RESOURCE_DESC) == 8);
static_assert(sizeof(D3D11_TEXTURE2D_DESC) == 44 && alignof(D3D11_TEXTURE2D_DESC) == 4);

namespace rsf_census {
namespace {

void* query_interface(void* object, const void* iid) noexcept {
    if (!vtable_slot(object, 0)) return nullptr;
    void* result = nullptr;
    HRESULT status = E_NOINTERFACE;
    if (!guard([&] {
            status = static_cast<IUnknown*>(object)->QueryInterface(*static_cast<const GUID*>(iid), &result);
        })) return nullptr;
    return SUCCEEDED(status) ? result : nullptr;
}

void release_interface(void* object) noexcept {
    if (vtable_slot(object, 2)) guard([&] { static_cast<IUnknown*>(object)->Release(); });
}

bool queue_desc(const void* queue, void* description) noexcept {
    return vtable_slot(queue, 18) && guard([&] {
        auto* object = static_cast<ID3D12CommandQueue*>(const_cast<void*>(queue));
        *static_cast<D3D12_COMMAND_QUEUE_DESC*>(description) = object->GetDesc();
    });
}

bool swapchain_desc(const void* chain, void* description) noexcept {
    if (!vtable_slot(chain, 12)) return false;
    HRESULT result = E_FAIL;
    return guard([&] {
        auto* object = static_cast<IDXGISwapChain*>(const_cast<void*>(chain));
        result = object->GetDesc(static_cast<DXGI_SWAP_CHAIN_DESC*>(description));
    }) && SUCCEEDED(result);
}

bool resource12_desc(const void* resource, void* description) noexcept {
    if (!vtable_slot(resource, 10)) return false;
    return guard([&] {
        auto* object = static_cast<ID3D12Resource*>(const_cast<void*>(resource));
        *static_cast<D3D12_RESOURCE_DESC*>(description) = object->GetDesc();
    });
}

bool resource11_dimension(const void* resource, int32_t* dimension) noexcept {
    if (!vtable_slot(resource, 10)) return false;
    return guard([&] {
        auto* object = static_cast<ID3D11Resource*>(const_cast<void*>(resource));
        D3D11_RESOURCE_DIMENSION value{};
        object->GetType(&value);
        *dimension = static_cast<int32_t>(value);
    });
}

bool resource11_desc(const void* resource, void* description) noexcept {
    return guard([&] {
        auto* object = static_cast<ID3D11Texture2D*>(const_cast<void*>(resource));
        object->GetDesc(static_cast<D3D11_TEXTURE2D_DESC*>(description));
    });
}

void enable_debug(int32_t* layer, int32_t* dred) noexcept {
    HMODULE d3d12 = GetModuleHandleW(L"d3d12.dll");
    using Get = HRESULT (WINAPI*)(REFIID, void**);
    auto get = d3d12 ? reinterpret_cast<Get>(
        reinterpret_cast<void*>(GetProcAddress(d3d12, "D3D12GetDebugInterface"))) : nullptr;
    *layer = *dred = E_NOINTERFACE;
    if (!get) return;
    ID3D12Debug* debug = nullptr;
    *layer = get(IID_PPV_ARGS(&debug));
    if (SUCCEEDED(*layer) && debug) {
        debug->EnableDebugLayer();
        debug->Release();
    }
    ID3D12DeviceRemovedExtendedDataSettings* settings = nullptr;
    *dred = get(IID_PPV_ARGS(&settings));
    if (SUCCEEDED(*dred) && settings) {
        settings->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
        settings->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
        settings->Release();
    }
}

std::atomic<rsf_census_debug_message> message_callback{nullptr};

void CALLBACK debug_message(D3D12_MESSAGE_CATEGORY category, D3D12_MESSAGE_SEVERITY severity,
                            D3D12_MESSAGE_ID id, LPCSTR text, void* device) {
    if (auto callback = message_callback.load(std::memory_order_acquire)) {
        callback(static_cast<int32_t>(category), static_cast<int32_t>(severity),
                 static_cast<int32_t>(id), text, device);
    }
}

int32_t info_queue(void* device, rsf_census_debug_message callback, uint32_t* cookie) noexcept {
    ID3D12InfoQueue1* queue = nullptr;
    auto result = static_cast<IUnknown*>(device)->QueryInterface(IID_PPV_ARGS(&queue));
    if (FAILED(result) || !queue) return result;
    queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_CORRUPTION, FALSE);
    queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_ERROR, FALSE);
    queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_WARNING, FALSE);
    message_callback.store(callback, std::memory_order_release);
    DWORD native_cookie = 0;
    result = queue->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_IGNORE_FILTERS,
                                           device, &native_cookie);
    *cookie = native_cookie;
    // Keep this interface as long as its registered callback can receive messages.
    if (FAILED(result)) queue->Release();
    return result;
}

} // namespace

void describe_d3d_resource(rsf_census_json* json, const char* key, const void* resource,
                           bool d3d12) noexcept {
    static const rsf_census_resource_reads reads = {
        resource12_desc, resource11_dimension, resource11_desc,
    };
    rsf_census_json_resource(json, key, resource, d3d12, &reads);
}

void register_wrappers_d3d(void** table) noexcept {
    static constexpr uint16_t entries[] = {
#define RSF_CENSUS_D3D_ID(name) static_cast<uint16_t>(Fn::name),
        RSF_CENSUS_D3D_ENTRIES(RSF_CENSUS_D3D_ID)
#undef RSF_CENSUS_D3D_ID
    };
    static_assert(std::size(entries) == static_cast<size_t>(RSF_D3D_COUNT));
    static const rsf_census_d3d_platform platform = {
        safe_read, safe_read_cstr, vtable_slot, query_interface, release_interface,
        queue_desc, swapchain_desc, enable_debug, info_queue,
        rsf_census_present_enter, rsf_census_present_leave,
    };
    void* wrappers[RSF_D3D_COUNT]{};
    rsf_census_d3d_bind(&platform, entries, wrappers);
    for (size_t i = 0; i < std::size(entries); ++i) {
        if (wrappers[i]) table[entries[i]] = wrappers[i];
    }
}

} // namespace rsf_census
