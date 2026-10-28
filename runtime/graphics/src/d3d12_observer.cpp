// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d12_observer.h>
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <MinHook.h>
#include <array>
#include <atomic>
#include <mutex>
#include <new>
#include <utility>
#include <cstdio>
#include <cstring>

struct rsf_d3d12_observer { rsf_d3d12_observer_setup setup{}; std::atomic<uint64_t> serial{}; };
namespace {
using Microsoft::WRL::ComPtr;
using Factory = HRESULT (STDMETHODCALLTYPE*)(IDXGIFactory2*, IUnknown*, HWND, const DXGI_SWAP_CHAIN_DESC1*,
    const DXGI_SWAP_CHAIN_FULLSCREEN_DESC*, IDXGIOutput*, IDXGISwapChain1**);
using Device = HRESULT (STDMETHODCALLTYPE*)(ID3D12Device*, const D3D12_COMMAND_QUEUE_DESC*, REFIID, void**);
using Queue = void (STDMETHODCALLTYPE*)(ID3D12CommandQueue*, UINT, ID3D12CommandList* const*);
using Present = HRESULT (STDMETHODCALLTYPE*)(IDXGISwapChain*, UINT, UINT);
template<class Function> struct Entry { void* target{}; Function original{}; };
constexpr size_t capacity = 8;
std::array<Entry<Factory>, capacity> factories;
std::array<Entry<Device>, capacity> devices;
std::array<Entry<Queue>, capacity> queues;
std::array<Entry<Present>, capacity> chains;
std::mutex binding;
rsf_d3d12_observer* owner{};
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
    unsigned& depth;
    bool outer;
    explicit CallScope(unsigned& value) : depth(value), outer(value++ == 0) {}
    ~CallScope() { --depth; }
};
thread_local unsigned factory_depth{}, queue_depth{}, present_depth{};
void attach_queue(ID3D12CommandQueue*);
void attach_chain(IDXGISwapChain1*);
bool accepted(HWND window)
{ return owner && owner->setup.accept_window && owner->setup.accept_window(owner->setup.user, window); }
template<size_t I> HRESULT STDMETHODCALLTYPE factory(IDXGIFactory2* self, IUnknown* queue, HWND window,
    const DXGI_SWAP_CHAIN_DESC1* desc, const DXGI_SWAP_CHAIN_FULLSCREEN_DESC* fullscreen,
    IDXGIOutput* output, IDXGISwapChain1** chain)
{
    CallScope scope(factory_depth);
    const auto result = factories[I].original(self, queue, window, desc, fullscreen, output, chain);
    if (scope.outer && SUCCEEDED(result) && chain && *chain && accepted(window)) {
        ComPtr<ID3D12CommandQueue> direct;
        if (SUCCEEDED(queue->QueryInterface(IID_PPV_ARGS(&direct))) && direct->GetDesc().Type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
            attach_queue(direct.Get()); attach_chain(*chain);
            if (owner->setup.swapchain) owner->setup.swapchain(owner->setup.user, direct.Get(), *chain, window);
        }
    }
    return result;
}
template<size_t I> HRESULT STDMETHODCALLTYPE device(ID3D12Device* self, const D3D12_COMMAND_QUEUE_DESC* desc,
    REFIID iid, void** result)
{
    const auto code = devices[I].original(self, desc, iid, result);
    if (SUCCEEDED(code) && result && *result) {
        ComPtr<ID3D12CommandQueue> queue;
        if (SUCCEEDED(static_cast<IUnknown*>(*result)->QueryInterface(IID_PPV_ARGS(&queue)))) attach_queue(queue.Get());
    }
    return code;
}
template<size_t I> void STDMETHODCALLTYPE queue(ID3D12CommandQueue* self, UINT count, ID3D12CommandList* const* lists)
{
    CallScope scope(queue_depth);
    if (!scope.outer) { queues[I].original(self, count, lists); return; }
    const auto id = ++owner->serial;
    const auto pointers = reinterpret_cast<void* const*>(lists);
    if (owner->setup.submit) owner->setup.submit(owner->setup.user, self, id, count, pointers, 1);
    queues[I].original(self, count, lists);
    if (owner->setup.submit) owner->setup.submit(owner->setup.user, self, id, count, pointers, 0);
    report_removal(self);
}
template<size_t I> HRESULT STDMETHODCALLTYPE present(IDXGISwapChain* self, UINT interval, UINT flags)
{
    CallScope scope(present_depth);
    if (!scope.outer) return chains[I].original(self, interval, flags);
    DXGI_SWAP_CHAIN_DESC description{};
    const bool selected = SUCCEEDED(self->GetDesc(&description)) && accepted(description.OutputWindow);
    if (selected && owner->setup.present) owner->setup.present(owner->setup.user, self, interval, flags, 1);
    const auto result = chains[I].original(self, interval, flags);
    if (selected && owner->setup.present) owner->setup.present(owner->setup.user, self, interval, flags, 0);
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
template<class Function> bool bind(std::array<Entry<Function>, capacity>& entries, const std::array<void*, capacity>& wrappers, void* target)
{
    if (!target) return false;
    std::lock_guard lock(binding);
    for (const auto& entry : entries) if (entry.target == target) return true;
    for (size_t i = 0; i < capacity; ++i) if (!entries[i].target) {
        void* trampoline{};
        if (MH_CreateHook(target, wrappers[i], &trampoline) != MH_OK) return false;
        entries[i].original = reinterpret_cast<Function>(trampoline);
        entries[i].target = target;
        if (MH_EnableHook(target) == MH_OK) return true;
        MH_RemoveHook(target); entries[i] = {}; return false;
    }
    return false;
}
void attach_queue(ID3D12CommandQueue* object)
{
    if (object->GetDesc().Type != D3D12_COMMAND_LIST_TYPE_DIRECT) return;
    const auto wrappers = queue_wrappers(std::make_index_sequence<capacity>{});
    (void)bind(queues, wrappers, (*reinterpret_cast<void***>(object))[10]);
}
void attach_chain(IDXGISwapChain1* object)
{
    const auto wrappers = present_wrappers(std::make_index_sequence<capacity>{});
    (void)bind(chains, wrappers, (*reinterpret_cast<void***>(object))[8]);
}
}
extern "C" int32_t rsf_d3d12_observer_create(const rsf_d3d12_observer_setup* setup, rsf_d3d12_observer** out)
{
    if (!setup || setup->struct_size < sizeof(*setup) || setup->abi_version != 1 || !setup->accept_window || !out) return -1;
    *out = nullptr;
    std::lock_guard lock(binding);
    if (owner) return -2;
    const auto initialized = MH_Initialize();
    if (initialized != MH_OK && initialized != MH_ERROR_ALREADY_INITIALIZED) return -3;
    owner = new(std::nothrow) rsf_d3d12_observer;
    if (!owner) return -3;
    owner->setup = *setup; *out = owner; return 0;
}
extern "C" int32_t rsf_d3d12_observer_device(rsf_d3d12_observer* observer, void* pointer)
{
    if (!observer || observer != owner || !pointer) return -1;
    ComPtr<ID3D12Device> object;
    if (FAILED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&object)))) return -1;
    if (diagnostic_log) {
        ComPtr<ID3D12InfoQueue1> info;
        DWORD cookie{};
        if (SUCCEEDED(object.As(&info))) (void)info->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie);
    }
    const auto wrappers = device_wrappers(std::make_index_sequence<capacity>{});
    return bind(devices, wrappers, (*reinterpret_cast<void***>(object.Get()))[8]) ? 0 : -3;
}
extern "C" int32_t rsf_d3d12_observer_factory(rsf_d3d12_observer* observer, void* pointer)
{
    if (!observer || observer != owner || !pointer) return -1;
    ComPtr<IDXGIFactory2> object;
    if (FAILED(static_cast<IUnknown*>(pointer)->QueryInterface(IID_PPV_ARGS(&object)))) return -1;
    const auto wrappers = factory_wrappers(std::make_index_sequence<capacity>{});
    return bind(factories, wrappers, (*reinterpret_cast<void***>(object.Get()))[15]) ? 0 : -3;
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
