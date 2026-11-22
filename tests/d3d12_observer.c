// SPDX-License-Identifier: MIT
#define COBJMACROS
#include <rescaleframe/d3d12_observer.h>
#include <windows.h>
#include <initguid.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <stdbool.h>
#include <stdio.h>

static HWND window;
static LONG submissions, completions, presents, chains;
static bool nested_present;

static int accept_window(void* user, void* candidate)
{
    (void)user;
    return candidate == window;
}

static void submit(void* user, void* queue, uint64_t id, uint32_t count,
                   void* const* lists, uint32_t begin)
{
    (void)user; (void)queue; (void)id; (void)lists;
    if (count) {
        InterlockedIncrement(begin ? &submissions : &completions);
    }
}

static void present(void* user, void* chain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    (void)user; (void)interval; (void)flags;
    if (!begin) return;
    InterlockedIncrement(&presents);
    if (!nested_present) {
        nested_present = true;
        // Exercise a reentrant Present through the hooked entry.
        (void)IDXGISwapChain_Present((IDXGISwapChain*)chain, 0, DXGI_PRESENT_TEST);
    }
}

static void swapchain_created(void* user, void* queue, void* chain, void* window_handle)
{
    (void)user; (void)queue; (void)chain; (void)window_handle;
    InterlockedIncrement(&chains);
}

static bool observe_warp(void)
{
    IDXGIFactory4* factory = NULL;
    IDXGIAdapter* adapter = NULL;
    ID3D12Device* device = NULL;
    ID3D12CommandQueue* queue = NULL;
    ID3D12CommandAllocator* allocator = NULL;
    ID3D12GraphicsCommandList* list = NULL;
    IDXGISwapChain1* swapchain = NULL;
    ID3D12Fence* fence = NULL;
    HANDLE event = NULL;
    bool passed = false;

    if (FAILED(CreateDXGIFactory1(&IID_IDXGIFactory4, (void**)&factory)) ||
        FAILED(IDXGIFactory4_EnumWarpAdapter(factory, &IID_IDXGIAdapter, (void**)&adapter)) ||
        FAILED(D3D12CreateDevice((IUnknown*)adapter, D3D_FEATURE_LEVEL_11_0,
                                 &IID_ID3D12Device, (void**)&device))) goto cleanup;
    const IDXGIFactory4Vtbl* factory_table = factory->lpVtbl;
    const ID3D12DeviceVtbl* device_table = device->lpVtbl;
    const rsf_d3d12_observer_setup setup = {
        sizeof(setup), 1, NULL, accept_window, submit, present, swapchain_created,
    };
    rsf_d3d12_observer* observer = NULL;
    if (rsf_d3d12_observer_create(&setup, &observer) ||
        rsf_d3d12_observer_factory(observer, factory) ||
        rsf_d3d12_observer_device(observer, device)) goto cleanup;
    if (factory->lpVtbl != factory_table || device->lpVtbl != device_table) goto cleanup;

    D3D12_COMMAND_QUEUE_DESC queue_desc = {0};
    queue_desc.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    if (FAILED(ID3D12Device_CreateCommandQueue(device, &queue_desc,
                &IID_ID3D12CommandQueue, (void**)&queue)) ||
        FAILED(ID3D12Device_CreateCommandAllocator(device, D3D12_COMMAND_LIST_TYPE_DIRECT,
                &IID_ID3D12CommandAllocator, (void**)&allocator)) ||
        FAILED(ID3D12Device_CreateCommandList(device, 0, D3D12_COMMAND_LIST_TYPE_DIRECT,
                allocator, NULL, &IID_ID3D12GraphicsCommandList, (void**)&list)) ||
        FAILED(ID3D12GraphicsCommandList_Close(list))) goto cleanup;
    ID3D12CommandList* lists[] = {(ID3D12CommandList*)list};
    ID3D12CommandQueue_ExecuteCommandLists(queue, 1, lists);

    DXGI_SWAP_CHAIN_DESC1 desc = {0};
    desc.Width = 320;
    desc.Height = 240;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1;
    desc.BufferCount = 2;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    if (FAILED(IDXGIFactory4_CreateSwapChainForHwnd(factory, (IUnknown*)queue, window,
                &desc, NULL, NULL, &swapchain)) ||
        FAILED(IDXGISwapChain1_Present(swapchain, 0, 0))) goto cleanup;
    if (FAILED(ID3D12Device_CreateFence(device, 0, D3D12_FENCE_FLAG_NONE,
                &IID_ID3D12Fence, (void**)&fence)) ||
        FAILED(ID3D12CommandQueue_Signal(queue, fence, 1))) goto cleanup;
    event = CreateEventW(NULL, FALSE, FALSE, NULL);
    if (!event || FAILED(ID3D12Fence_SetEventOnCompletion(fence, 1, event)) ||
        WaitForSingleObject(event, 10000) != WAIT_OBJECT_0) goto cleanup;
    passed = submissions > 0 && submissions == completions && presents == 1 && chains == 1;

cleanup:
    if (event) CloseHandle(event);
    if (fence) ID3D12Fence_Release(fence);
    if (swapchain) IDXGISwapChain1_Release(swapchain);
    if (list) ID3D12GraphicsCommandList_Release(list);
    if (allocator) ID3D12CommandAllocator_Release(allocator);
    if (queue) ID3D12CommandQueue_Release(queue);
    if (device) ID3D12Device_Release(device);
    if (adapter) IDXGIAdapter_Release(adapter);
    if (factory) IDXGIFactory4_Release(factory);
    return passed;
}

int main(void)
{
    if (!rsf_d3d12_observer_device(NULL, (void*)(uintptr_t)1) ||
        !rsf_d3d12_observer_factory(NULL, (void*)(uintptr_t)1)) return 1;
    WNDCLASSW klass = {0};
    klass.lpfnWndProc = DefWindowProcW;
    klass.hInstance = GetModuleHandleW(NULL);
    klass.lpszClassName = L"RSFObserverContract";
    if (!RegisterClassW(&klass)) return 1;
    window = CreateWindowW(klass.lpszClassName, L"", WS_OVERLAPPEDWINDOW, 0, 0, 320, 240,
                           NULL, NULL, klass.hInstance, NULL);
    const bool passed = window && observe_warp();
    if (window) DestroyWindow(window);
    UnregisterClassW(klass.lpszClassName, klass.hInstance);
    if (!passed) return 1;
    puts("D3D12 observer: WARP creation/submission/Present, unchanged vtables and native destruction passed.");
    return 0;
}
