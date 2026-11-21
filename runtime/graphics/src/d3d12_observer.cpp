// SPDX-License-Identifier: MIT
#include "d3d12_observer_platform.h"
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <array>
#include <utility>

namespace {
using Microsoft::WRL::ComPtr;
using Factory = HRESULT (STDMETHODCALLTYPE*)(IDXGIFactory2*, IUnknown*, HWND, const DXGI_SWAP_CHAIN_DESC1*,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
using Device = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
using Queue = void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using Present = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
constexpr size_t capacity = RSF_OBSERVER12_CAPACITY;
static_assert(sizeof(rsf_observer12_hooks) == 4 * sizeof(void*));
static_assert(sizeof(rsf_observer12_attachment) == 7 * sizeof(void*));
static_assert(RSF_OBSERVER12_FACTORY == 0 && RSF_OBSERVER12_DEVICE == 1 && RSF_OBSERVER12_QUEUE == 2 &&
    RSF_OBSERVER12_CHAIN == 3 && D3D12_COMMAND_LIST_TYPE_DIRECT == 0);
static_assert(sizeof(rsf_observer12_breadcrumb) == 40 && offsetof(rsf_observer12_breadcrumb, name) == 32);
static_assert(sizeof(rsf_observer12_diagnostic_setup) == 5 * sizeof(void*) &&
    sizeof(rsf_observer12_removal) == 7 * sizeof(void*));
static_assert(D3D12_MESSAGE_SEVERITY_ERROR == 1 && D3D12_MESSAGE_SEVERITY_WARNING == 2);
constexpr rsf_observer12_hooks hooks{
    []() -> int32_t {
        const auto status = MH_Initialize();
        return status == MH_OK || status == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](void* target, void* detour, void** original) -> int32_t {
        return MH_CreateHook(target, detour, original) == MH_OK;
    },
    [](void* target) -> int32_t { return MH_EnableHook(target) == MH_OK; },
    [](void* target) { (void)MH_RemoveHook(target); },
};
void CALLBACK debug_message(D3D12_MESSAGE_CATEGORY, D3D12_MESSAGE_SEVERITY severity,
    D3D12_MESSAGE_ID id, LPCSTR description, void*)
{
    rsf_core_observer12_debug_message(uint32_t(severity), uint32_t(id), description);
}
template<class Interface> int32_t query_interface(void* object, void** out) noexcept try
{
    return static_cast<IUnknown*>(object)->QueryInterface(__uuidof(Interface), out);
} catch (...) { return E_FAIL; }

template<class Interface> int32_t query_debug(void** out) noexcept try
{
    return D3D12GetDebugInterface(__uuidof(Interface), out);
} catch (...) { return E_FAIL; }

int32_t queue_device(void* queue, void** out) noexcept try
{
    return static_cast<ID3D12CommandQueue*>(queue)->GetDevice(__uuidof(ID3D12Device), out);
} catch (...) { return E_FAIL; }

int32_t removed_reason(void* device) noexcept try
{
    return static_cast<ID3D12Device*>(device)->GetDeviceRemovedReason();
} catch (...) { return S_OK; }

const void* breadcrumb_head(void* dred) noexcept try
{
    D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT output{};
    const auto result = static_cast<ID3D12DeviceRemovedExtendedData*>(dred)->GetAutoBreadcrumbsOutput(&output);
    return SUCCEEDED(result) ? output.pHeadAutoBreadcrumbNode : nullptr;
} catch (...) { return nullptr; }

void breadcrumb(const void* raw, rsf_observer12_breadcrumb* out) noexcept
{
    const auto& node = *static_cast<const D3D12_AUTO_BREADCRUMB_NODE*>(raw);
    *out = {node.pNext, node.pCommandList, node.pCommandQueue,
        node.pLastBreadcrumbValue ? *node.pLastBreadcrumbValue : 0, node.BreadcrumbCount, node.pCommandListDebugNameA};
}

uint32_t breadcrumb_operation(const void* raw, uint32_t index) noexcept
{
    const auto& node = *static_cast<const D3D12_AUTO_BREADCRUMB_NODE*>(raw);
    return node.pCommandHistory ? static_cast<uint32_t>(node.pCommandHistory[index]) : UINT32_MAX;
}

int32_t page_fault(void* dred, uint64_t* address) noexcept try
{
    D3D12_DRED_PAGE_FAULT_OUTPUT output{};
    const auto result = static_cast<ID3D12DeviceRemovedExtendedData*>(dred)->GetPageFaultAllocationOutput(&output);
    if (SUCCEEDED(result)) *address = output.PageFaultVA;
    return result;
} catch (...) { return E_FAIL; }

constexpr rsf_observer12_diagnostic_setup diagnostic_setup{
    query_debug<ID3D12DeviceRemovedExtendedDataSettings>,
    [](void* settings) noexcept {
        try {
            static_cast<ID3D12DeviceRemovedExtendedDataSettings*>(settings)->SetAutoBreadcrumbsEnablement(
                D3D12_DRED_ENABLEMENT_FORCED_ON);
        } catch (...) {}
    },
    [](void* settings) noexcept {
        try {
            static_cast<ID3D12DeviceRemovedExtendedDataSettings*>(settings)->SetPageFaultEnablement(
                D3D12_DRED_ENABLEMENT_FORCED_ON);
        } catch (...) {}
    },
    query_debug<ID3D12Debug>,
    [](void* debug) noexcept -> int32_t {
        try {
            static_cast<ID3D12Debug*>(debug)->EnableDebugLayer();
            return 1;
        } catch (...) { return 0; }
    },
};

constexpr rsf_observer12_removal removal{
    queue_device, removed_reason, query_interface<ID3D12DeviceRemovedExtendedData>,
    breadcrumb_head, breadcrumb, breadcrumb_operation, page_fault,
};

// SDK proxies can forward through another observed implementation. Each public
// operation has one callback pair, including its native forwarding chain.
struct CallScope {
    uint32_t kind;
    bool outer;
    explicit CallScope(uint32_t kind_) : kind(kind_), outer(rsf_core_observer12_enter(kind) != 0) {}
    ~CallScope() { rsf_core_observer12_leave(kind); }
};
template<class Method> Method original(uint32_t kind, size_t index)
{ return reinterpret_cast<Method>(rsf_core_observer12_original(kind, index)); }

extern const rsf_observer12_attachment attachment;
template<size_t I> HRESULT STDMETHODCALLTYPE factory(IDXGIFactory2* self, IUnknown* queue, HWND window,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen,
    IDXGIOutput* output, IDXGISwapChain1** chain)
{
    CallScope scope(RSF_OBSERVER12_FACTORY);
    const auto result = original<Factory>(RSF_OBSERVER12_FACTORY, I)(self, queue, window, desc, fullscreen, output, chain);
    rsf_core_observer12_factory_result(&attachment, result, scope.outer, queue, window,
        reinterpret_cast<void* const*>(chain));
    return result;
}
template<size_t I> HRESULT STDMETHODCALLTYPE device(ID3D12Device* self, const D3D12_COMMAND_QUEUE_DESC* desc,
    REFIID iid, void** result)
{
    const auto code = original<Device>(RSF_OBSERVER12_DEVICE, I)(self, desc, iid, result);
    rsf_core_observer12_device_result(&attachment, code, result);
    return code;
}
template<size_t I> void STDMETHODCALLTYPE queue(ID3D12CommandQueue* self, UINT count, ID3D12CommandList* const* lists)
{
    CallScope scope(RSF_OBSERVER12_QUEUE);
    const auto call = original<Queue>(RSF_OBSERVER12_QUEUE, I);
    if (!scope.outer) { call(self, count, lists); return; }
    const auto pointers = reinterpret_cast<void* const*>(lists);
    const auto id = rsf_core_observer12_submit_begin(self, count, pointers);
    call(self, count, lists);
    rsf_core_observer12_submit_end(self, id, count, pointers);
    rsf_core_observer12_report_removal(self, &removal);
}
template<size_t I> HRESULT STDMETHODCALLTYPE present(IDXGISwapChain* self, UINT interval, UINT flags)
{
    CallScope scope(RSF_OBSERVER12_CHAIN);
    const auto call = original<Present>(RSF_OBSERVER12_CHAIN, I);
    if (!scope.outer) return call(self, interval, flags);
    const bool selected = rsf_core_observer12_present_begin(&attachment, self, interval, flags) != 0;
    const auto result = call(self, interval, flags);
    if (selected) rsf_core_observer12_present(self, interval, flags, 0);
    return result;
}
template<size_t... I> auto factory_wrappers(std::index_sequence<I...>)
{ return std::array<void*, sizeof...(I)>{reinterpret_cast<void*>(&factory<I>)...}; }
template<size_t... I> auto device_wrappers(std::index_sequence<I...>)
{ return std::array<void*, sizeof...(I)>{reinterpret_cast<void*>(&device<I>)...}; }
template<size_t... I> auto queue_wrappers(std::index_sequence<I...>)
{ return std::array<void*, sizeof...(I)>{reinterpret_cast<void*>(&queue<I>)...}; }
template<size_t... I> auto present_wrappers(std::index_sequence<I...>)
{ return std::array<void*, sizeof...(I)>{reinterpret_cast<void*>(&present<I>)...}; }
int32_t bind_interface(uint32_t kind, void* object, size_t slot) noexcept try
{
    const auto target = (*reinterpret_cast<void***>(object))[slot];
    const auto indices = std::make_index_sequence<capacity>{};
    switch (kind) {
    case RSF_OBSERVER12_FACTORY:
        return rsf_core_observer12_bind(kind, target, factory_wrappers(indices).data(), &hooks);
    case RSF_OBSERVER12_DEVICE:
        return rsf_core_observer12_bind(kind, target, device_wrappers(indices).data(), &hooks);
    case RSF_OBSERVER12_QUEUE:
        return rsf_core_observer12_bind(kind, target, queue_wrappers(indices).data(), &hooks);
    case RSF_OBSERVER12_CHAIN:
        return rsf_core_observer12_bind(kind, target, present_wrappers(indices).data(), &hooks);
    default:
        return 0;
    }
} catch (...) { return 0; }

uint32_t queue_type(void* queue) noexcept try
{
    return static_cast<uint32_t>(static_cast<ID3D12CommandQueue*>(queue)->GetDesc().Type);
} catch (...) { return UINT32_MAX; }

void register_debug(void* object) noexcept try
{
    ComPtr<ID3D12InfoQueue1> info;
    DWORD cookie{};
    if (SUCCEEDED(static_cast<ID3D12Device*>(object)->QueryInterface(IID_PPV_ARGS(&info))) && info)
        (void)info->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie);
} catch (...) {}

int32_t describe_window(void* chain, void** window) noexcept try
{
    DXGI_SWAP_CHAIN_DESC description{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&description))) return 0;
    *window = description.OutputWindow;
    return 1;
} catch (...) { return 0; }

const rsf_observer12_attachment attachment{
    query_interface<IDXGIFactory2>, query_interface<ID3D12Device>, query_interface<ID3D12CommandQueue>,
    queue_type, bind_interface, register_debug, describe_window,
};
}
extern "C" int32_t rsf_d3d12_observer_create(const rsf_d3d12_observer_setup* setup, rsf_d3d12_observer** out)
{
    return rsf_core_observer12_create(setup, out, &hooks);
}
extern "C" int32_t rsf_d3d12_observer_device(rsf_d3d12_observer* observer, void* pointer)
{
    return rsf_core_observer12_attach(observer, pointer, RSF_OBSERVER12_DEVICE, &attachment);
}
extern "C" int32_t rsf_d3d12_observer_factory(rsf_d3d12_observer* observer, void* pointer)
{
    return rsf_core_observer12_attach(observer, pointer, RSF_OBSERVER12_FACTORY, &attachment);
}

extern "C" int32_t rsf_d3d12_diagnostics_enable(void (*log)(void*, const char*), void* user)
{
    return rsf_core_observer12_diagnostics_enable(log, user, &diagnostic_setup);
}
