// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/d3d12_observer.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <atomic>
#include <cstdio>

namespace {
using Microsoft::WRL::ComPtr;
HWND window{};
std::atomic<uint32_t> submissions{}, completions{}, presents{}, chains{};
bool nested_present{};
int accept(void*, void* candidate) { return candidate == window; }
void submit(void*, void*, uint64_t, uint32_t count, void* const*, uint32_t begin)
{ if (count) { if (begin) ++submissions; else ++completions; } }
void present(void*, void* chain, uint32_t, uint32_t, uint32_t begin)
{
    if (!begin) return;
    ++presents;
    if (!nested_present) {
        nested_present = true;
        // Exercise a forwarding/reentrant Present through the hooked entry.
        (void)static_cast<IDXGISwapChain*>(chain)->Present(0, DXGI_PRESENT_TEST);
    }
}
void chain(void*, void*, void*, void*) { ++chains; }
}
int main()
{
    if (!rsf_d3d12_observer_device(nullptr, reinterpret_cast<void*>(1)) ||
        !rsf_d3d12_observer_factory(nullptr, reinterpret_cast<void*>(1))) return 1;
    const auto instance = GetModuleHandleW(nullptr);
    WNDCLASSW klass{}; klass.lpfnWndProc = DefWindowProcW; klass.hInstance = instance; klass.lpszClassName = L"RSFObserverContract";
    if (!RegisterClassW(&klass)) return 1;
    window = CreateWindowW(klass.lpszClassName, L"", WS_OVERLAPPEDWINDOW, 0, 0, 320, 240, nullptr, nullptr, instance, nullptr);
    if (!window) return 1;
    {
        ComPtr<IDXGIFactory4> factory;
        ComPtr<IDXGIAdapter> adapter;
        ComPtr<ID3D12Device> device;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->EnumWarpAdapter(IID_PPV_ARGS(&adapter))) ||
            FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return 1;
        auto** factory_table = *reinterpret_cast<void***>(factory.Get());
        auto** device_table = *reinterpret_cast<void***>(device.Get());
        const rsf_d3d12_observer_setup setup{sizeof(setup),1,nullptr,accept,submit,present,chain};
        rsf_d3d12_observer* observer{};
        if (rsf_d3d12_observer_create(&setup,&observer) || rsf_d3d12_observer_factory(observer,factory.Get()) ||
            rsf_d3d12_observer_device(observer,device.Get())) return 1;
        if (*reinterpret_cast<void***>(factory.Get()) != factory_table || *reinterpret_cast<void***>(device.Get()) != device_table) return 1;
        D3D12_COMMAND_QUEUE_DESC description{}; description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        ComPtr<ID3D12CommandQueue> queue;
        ComPtr<ID3D12CommandAllocator> allocator;
        ComPtr<ID3D12GraphicsCommandList> list;
        if (FAILED(device->CreateCommandQueue(&description,IID_PPV_ARGS(&queue))) ||
            FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT,IID_PPV_ARGS(&allocator))) ||
            FAILED(device->CreateCommandList(0,D3D12_COMMAND_LIST_TYPE_DIRECT,allocator.Get(),nullptr,IID_PPV_ARGS(&list))) || FAILED(list->Close())) return 1;
        ID3D12CommandList* lists[]{list.Get()}; queue->ExecuteCommandLists(1,lists);
        DXGI_SWAP_CHAIN_DESC1 desc{};desc.Width=320;desc.Height=240;desc.Format=DXGI_FORMAT_R8G8B8A8_UNORM;
        desc.SampleDesc.Count=1;desc.BufferCount=2;desc.BufferUsage=DXGI_USAGE_RENDER_TARGET_OUTPUT;desc.SwapEffect=DXGI_SWAP_EFFECT_FLIP_DISCARD;
        ComPtr<IDXGISwapChain1> swapchain;
        if (FAILED(factory->CreateSwapChainForHwnd(queue.Get(),window,&desc,nullptr,nullptr,&swapchain)) ||
            FAILED(swapchain->Present(0,0))) return 1;
        ComPtr<ID3D12Fence> fence;
        if (FAILED(device->CreateFence(0,D3D12_FENCE_FLAG_NONE,IID_PPV_ARGS(&fence))) || FAILED(queue->Signal(fence.Get(),1))) return 1;
        const HANDLE event=CreateEventW(nullptr,FALSE,FALSE,nullptr);
        if (!event) return 1;
        const bool joined=SUCCEEDED(fence->SetEventOnCompletion(1,event)) && WaitForSingleObject(event,10000)==WAIT_OBJECT_0;
        CloseHandle(event);
        if (!joined || !submissions || submissions!=completions || presents!=1 || chains!=1) return 1;
        // Scope exit exercises native COM destruction with its private tables intact.
    }
    DestroyWindow(window);UnregisterClassW(klass.lpszClassName,instance);
    std::puts("D3D12 observer: WARP creation/submission/Present, unchanged vtables and native destruction passed.");
    return 0;
}
