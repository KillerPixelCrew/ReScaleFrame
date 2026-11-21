// SPDX-License-Identifier: MIT
#include "d3d12_observer_platform.h"
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <utility>
#include <cstdio>
#include <cstring>

namespace {
using Microsoft::WRL::ComPtr;
using Factory = HRESULT (STDMETHODCALLTYPE*)(IDXGIFactory2*, IUnknown*, HWND, const DXGI_SWAP_CHAIN_DESC1*,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
using Device = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
using Queue = void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using Present = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
constexpr size_t capacity = RSF_OBSERVER12_CAPACITY;
static_assert(sizeof(rsf_observer12_hooks) == 4 * sizeof(void*));
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
void (*diagnostic_log)(void*, const char*){};
void* diagnostic_user{};
std::atomic<bool> removal_reported{};
std::array<std::atomic<uint32_t>, 4096> diagnostic_counts{};
void CALLBACK debug_message(D3D12_MESSAGE_CATEGORY, D3D12_MESSAGE_SEVERITY severity,
    D3D12_MESSAGE_ID id, LPCSTR description, void*)
{
    if (!diagnostic_log || severity > D3D12_MESSAGE_SEVERITY_WARNING) return;
    const auto key = static_cast<unsigned>(id);
    if (key >= diagnostic_counts.size()) return;
    const auto occurrence = ++diagnostic_counts[key];
    if (occurrence > (severity == D3D12_MESSAGE_SEVERITY_WARNING ? 4u : 16u)) return;
    char message[2048]{};
    std::snprintf(message, sizeof(message), "D3D12 validation severity=%u id=%u: %s", unsigned(severity), unsigned(id), description);
    diagnostic_log(diagnostic_user, message);
    if (severity <= D3D12_MESSAGE_SEVERITY_ERROR && occurrence <= 2) {
        void* stack[16]{};
        const auto count = CaptureStackBackTrace(1, 16, stack, nullptr);
        for (USHORT i = 0; i < count; ++i) {
            HMODULE module{}; char path[MAX_PATH]{};
            if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    static_cast<const char*>(stack[i]), &module)) continue;
            (void)GetModuleFileNameA(module, path, MAX_PATH);
            const auto* leaf = std::strrchr(path, '\\');
            std::snprintf(message, sizeof(message), "D3D12 validation stack[%u] %s+0x%llx.", unsigned(i), leaf ? leaf + 1 : path,
                static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(stack[i]) - reinterpret_cast<uintptr_t>(module)));
            diagnostic_log(diagnostic_user, message);
        }
    }
}
void report_removal(ID3D12CommandQueue* queue)
{
    if (!diagnostic_log || removal_reported.load()) return;
    ComPtr<ID3D12Device> device;
    if (FAILED(queue->GetDevice(IID_PPV_ARGS(&device)))) return;
    const auto reason = device->GetDeviceRemovedReason();
    if (SUCCEEDED(reason) || removal_reported.exchange(true)) return;
    char message[512]{};
    std::snprintf(message, sizeof(message), "D3D12 device removal reason=0x%08lx.", static_cast<unsigned long>(reason));
    diagnostic_log(diagnostic_user, message);
    ComPtr<ID3D12DeviceRemovedExtendedData> dred;
    if (FAILED(device.As(&dred))) return;
    D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT breadcrumbs{};
    if (SUCCEEDED(dred->GetAutoBreadcrumbsOutput(&breadcrumbs))) {
        auto* node = breadcrumbs.pHeadAutoBreadcrumbNode;
        for (unsigned i = 0; node && i < 64; ++i, node = node->pNext) {
            const auto completed = node->pLastBreadcrumbValue ? *node->pLastBreadcrumbValue : 0;
            std::snprintf(message, sizeof(message), "DRED list=%p queue=%p completed=%u/%u next_operation=%u name=%s.",
                static_cast<void*>(node->pCommandList), static_cast<void*>(node->pCommandQueue), completed, node->BreadcrumbCount,
                completed < node->BreadcrumbCount && node->pCommandHistory ? unsigned(node->pCommandHistory[completed]) : UINT_MAX,
                node->pCommandListDebugNameA ? node->pCommandListDebugNameA : "");
            diagnostic_log(diagnostic_user, message);
        }
    }
    D3D12_DRED_PAGE_FAULT_OUTPUT fault{};
    if (SUCCEEDED(dred->GetPageFaultAllocationOutput(&fault))) {
        std::snprintf(message, sizeof(message), "DRED page fault address=0x%llx.", static_cast<unsigned long long>(fault.PageFaultVA));
        diagnostic_log(diagnostic_user, message);
    }
}
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

void attach_queue(ID3D12CommandQueue*);
void attach_chain(IDXGISwapChain1*);
template<size_t I> HRESULT STDMETHODCALLTYPE factory(IDXGIFactory2* self, IUnknown* queue, HWND window,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen,
    IDXGIOutput* output, IDXGISwapChain1** chain)
{
    CallScope scope(RSF_OBSERVER12_FACTORY);
    const auto result = original<Factory>(RSF_OBSERVER12_FACTORY, I)(self, queue, window, desc, fullscreen, output, chain);
    if (scope.outer && SUCCEEDED(result) && queue && chain && *chain && rsf_core_observer12_accept(window)) {
        ComPtr<ID3D12CommandQueue> direct;
        if (SUCCEEDED(queue->QueryInterface(IID_PPV_ARGS(&direct))) && direct->GetDesc().Type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
            attach_queue(direct.Get()); attach_chain(*chain);
            rsf_core_observer12_swapchain(direct.Get(), *chain, window);
        }
    }
    return result;
}
template<size_t I> HRESULT STDMETHODCALLTYPE device(ID3D12Device* self, const D3D12_COMMAND_QUEUE_DESC* desc,
    REFIID iid, void** result)
{
    const auto code = original<Device>(RSF_OBSERVER12_DEVICE, I)(self, desc, iid, result);
    if (SUCCEEDED(code) && result && *result) {
        ComPtr<ID3D12CommandQueue> queue;
        if (SUCCEEDED(static_cast<IUnknown*>(*result)->QueryInterface(IID_PPV_ARGS(&queue)))) attach_queue(queue.Get());
    }
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
    report_removal(self);
}
template<size_t I> HRESULT STDMETHODCALLTYPE present(IDXGISwapChain* self, UINT interval, UINT flags)
{
    CallScope scope(RSF_OBSERVER12_CHAIN);
    const auto call = original<Present>(RSF_OBSERVER12_CHAIN, I);
    if (!scope.outer) return call(self, interval, flags);
    DXGI_SWAP_CHAIN_DESC description{};
    const bool selected = SUCCEEDED(self->GetDesc(&description)) && rsf_core_observer12_accept(description.OutputWindow);
    if (selected) rsf_core_observer12_present(self, interval, flags, 1);
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
bool bind(uint32_t kind, const std::array<void*, capacity>& wrappers, void* target)
{
    return rsf_core_observer12_bind(kind, target, wrappers.data(), &hooks) != 0;
}
void attach_queue(ID3D12CommandQueue* object)
{
    if (object->GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT) return;
    const auto wrappers = queue_wrappers(std::make_index_sequence<capacity>{});
    (void)bind(RSF_OBSERVER12_QUEUE, wrappers, (*reinterpret_cast<void***>(object))[10]);
}
void attach_chain(IDXGISwapChain1* object)
{
    const auto wrappers = present_wrappers(std::make_index_sequence<capacity>{});
    (void)bind(RSF_OBSERVER12_CHAIN, wrappers, (*reinterpret_cast<void***>(object))[8]);
}
}
extern "C" int32_t rsf_d3d12_observer_create(const rsf_d3d12_observer_setup* setup, rsf_d3d12_observer** out)
{
    return rsf_core_observer12_create(setup, out, &hooks);
}
extern "C" int32_t rsf_d3d12_observer_device(rsf_d3d12_observer* observer, void* pointer)
{
    if (!pointer || !rsf_core_observer12_owns(observer)) return -1;
    ComPtr<ID3D12Device> object;
    if (FAILED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&object)))) return -1;
    if (diagnostic_log) {
        ComPtr<ID3D12InfoQueue1> info;
        DWORD cookie{};
        if (SUCCEEDED(object.As(&info))) (void)info->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie);
    }
    const auto wrappers = device_wrappers(std::make_index_sequence<capacity>{});
    return bind(RSF_OBSERVER12_DEVICE, wrappers, (*reinterpret_cast<void***>(object.Get()))[8]) ? 0 : -3;
}
extern "C" int32_t rsf_d3d12_observer_factory(rsf_d3d12_observer* observer, void* pointer)
{
    if (!pointer || !rsf_core_observer12_owns(observer)) return -1;
    ComPtr<IDXGIFactory2> object;
    if (FAILED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&object)))) return -1;
    const auto wrappers = factory_wrappers(std::make_index_sequence<capacity>{});
    return bind(RSF_OBSERVER12_FACTORY, wrappers, (*reinterpret_cast<void***>(object.Get()))[15]) ? 0 : -3;
}

extern "C" int32_t rsf_d3d12_diagnostics_enable(void (*log)(void*, const char*), void* user)
{
    if (!log) return -1;
    diagnostic_log = log; diagnostic_user = user;
    ComPtr<ID3D12DeviceRemovedExtendedDataSettings> dred;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&dred)))) {
        dred->SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
        dred->SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
    }
    ComPtr<ID3D12Debug> debug;
    if (FAILED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) {
        log(user, "D3D12 diagnostics: debug layer unavailable; DRED requested."); return -2;
    }
    debug->EnableDebugLayer();
    log(user, "D3D12 diagnostics: validation layer and DRED requested before device creation.");
    return 0;
}
