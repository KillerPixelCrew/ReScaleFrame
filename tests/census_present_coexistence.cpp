// Reproduces the census Present interaction with an injected overlay (RivaTuner) on real hardware.
//
// A visible D3D12 flip-discard swapchain renders frames the way RDR2 does: clear the current back
// buffer, execute, present, wait for the GPU. The D3D12 debug layer reports any command list that
// writes more than one back buffer of the swapchain (message 904) or other errors. The test runs
// with or without the vendor census installed, chosen by the first argument, so the same machine
// can show whether the census changes the overlay's behaviour.
//
// Opt-in: it needs a GPU and a desktop session, and an overlay injected into the process for the
// interaction to exist. Run: rsf_census_present_coexistence census|plain [frames]
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <dxgi1_6.h>
#include <windows.h>

#include <atomic>
#include <utility>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#include "rescaleframe/vendor_census.h"

namespace {
std::atomic<unsigned> g_errors{0};
std::atomic<unsigned> g_multi_buffer{0};

void CALLBACK on_message(D3D12_MESSAGE_CATEGORY, D3D12_MESSAGE_SEVERITY severity, D3D12_MESSAGE_ID id,
                         LPCSTR text, void*) {
    if (severity > D3D12_MESSAGE_SEVERITY_ERROR) return;
    if (id == 904) g_multi_buffer.fetch_add(1);
    if (g_errors.fetch_add(1) < 8) std::fprintf(stderr, "d3d12 error %d: %s\n", static_cast<int>(id), text);
}

template <class T> void release(T*& p) {
    if (p) p->Release();
    p = nullptr;
}
} // namespace

int main(int argc, char** argv) {
    const bool with_census = argc > 1 && !std::strcmp(argv[1], "census");
    const unsigned frames = argc > 2 ? static_cast<unsigned>(std::atoi(argv[2])) : 900;

    if (with_census) {
        char dir[MAX_PATH];
        GetTempPathA(MAX_PATH, dir);
        std::strcat(dir, "rsf-census-coexistence");
        rsf_census_options options{};
        options.struct_size = sizeof(options);
        options.abi_version = RSF_CENSUS_ABI_VERSION;
        options.output_dir_utf8 = dir;
        if (rsf_census_install(&options) != RSF_CENSUS_OK) {
            std::fprintf(stderr, "census install failed\n");
            return 2;
        }
        std::printf("census installed, output %s\n", dir);
    }

    ID3D12Debug* debug = nullptr;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    release(debug);

    WNDCLASSW wc{};
    wc.lpfnWndProc = DefWindowProcW;
    wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"RsfCensusCoexistence";
    RegisterClassW(&wc);
    HWND window = CreateWindowExW(0, wc.lpszClassName, L"census coexistence", WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                                  100, 100, 1280, 720, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 2;

    IDXGIFactory4* factory = nullptr;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return 2;
    ID3D12Device* device = nullptr;
    IDXGIAdapter1* adapter = nullptr;
    for (UINT i = 0; factory->EnumAdapters1(i, &adapter) != DXGI_ERROR_NOT_FOUND; ++i) {
        DXGI_ADAPTER_DESC1 d{};
        adapter->GetDesc1(&d);
        // The overlay only matters on the adapter the game renders with; prefer a discrete GPU.
        if (!(d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) && d.DedicatedVideoMemory > (512ull << 20) &&
            SUCCEEDED(D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) {
            std::printf("adapter %ls\n", d.Description);
            break;
        }
        release(adapter);
    }
    if (!device) {
        std::fprintf(stderr, "no hardware D3D12 adapter\n");
        return 2;
    }
    ID3D12InfoQueue1* info = nullptr;
    DWORD cookie = 0;
    if (SUCCEEDED(device->QueryInterface(IID_PPV_ARGS(&info)))) {
        info->RegisterMessageCallback(on_message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &cookie);
    }

    D3D12_COMMAND_QUEUE_DESC qd{};
    qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12CommandQueue* queue = nullptr;
    device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue));

    DXGI_SWAP_CHAIN_DESC1 sd{};
    sd.Width = 1280;
    sd.Height = 720;
    sd.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
    sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT | DXGI_USAGE_SHADER_INPUT;
    sd.BufferCount = 3;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    sd.Flags = DXGI_SWAP_CHAIN_FLAG_ALLOW_MODE_SWITCH;
    IDXGISwapChain1* sc1 = nullptr;
    if (FAILED(factory->CreateSwapChainForHwnd(queue, window, &sd, nullptr, nullptr, &sc1))) return 2;
    IDXGISwapChain3* swapchain = nullptr;
    sc1->QueryInterface(IID_PPV_ARGS(&swapchain));
    release(sc1);

    D3D12_DESCRIPTOR_HEAP_DESC hd{};
    hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV;
    hd.NumDescriptors = 3;
    ID3D12DescriptorHeap* rtvs = nullptr;
    device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&rtvs));
    const UINT step = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
    ID3D12Resource* buffers[3] = {};
    for (UINT i = 0; i < 3; ++i) {
        swapchain->GetBuffer(i, IID_PPV_ARGS(&buffers[i]));
        D3D12_CPU_DESCRIPTOR_HANDLE h = rtvs->GetCPUDescriptorHandleForHeapStart();
        h.ptr += static_cast<SIZE_T>(i) * step;
        device->CreateRenderTargetView(buffers[i], nullptr, h);
    }
    ID3D12CommandAllocator* allocators[3] = {};
    for (auto& a : allocators) device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&a));
    ID3D12GraphicsCommandList* list = nullptr;
    device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocators[0], nullptr, IID_PPV_ARGS(&list));
    list->Close();
    ID3D12Fence* fence = nullptr;
    device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence));
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    UINT64 fence_values[3] = {};
    UINT64 next = 1;
    unsigned reset_failures = 0, present_failures = 0;

    for (unsigned frame = 0; frame < frames; ++frame) {
        MSG msg;
        while (PeekMessageW(&msg, nullptr, 0, 0, PM_REMOVE)) DispatchMessageW(&msg);
        const UINT index = swapchain->GetCurrentBackBufferIndex();
        if (fence->GetCompletedValue() < fence_values[index]) {
            fence->SetEventOnCompletion(fence_values[index], event);
            WaitForSingleObject(event, 5000);
        }
        if (FAILED(allocators[index]->Reset()) || FAILED(list->Reset(allocators[index], nullptr))) {
            ++reset_failures;
            break;
        }
        D3D12_RESOURCE_BARRIER b{};
        b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        b.Transition.pResource = buffers[index];
        b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        b.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
        b.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
        list->ResourceBarrier(1, &b);
        D3D12_CPU_DESCRIPTOR_HANDLE h = rtvs->GetCPUDescriptorHandleForHeapStart();
        h.ptr += static_cast<SIZE_T>(index) * step;
        const float colour[4] = {0.1f, 0.2f + 0.3f * static_cast<float>(frame % 60) / 60.0f, 0.4f, 1.0f};
        list->ClearRenderTargetView(h, colour, 0, nullptr);
        std::swap(b.Transition.StateBefore, b.Transition.StateAfter);
        list->ResourceBarrier(1, &b);
        list->Close();
        ID3D12CommandList* lists[] = {list};
        queue->ExecuteCommandLists(1, lists);
        if (FAILED(swapchain->Present(1, 0))) ++present_failures;
        fence_values[index] = next;
        queue->Signal(fence, next++);
    }
    queue->Signal(fence, next);
    fence->SetEventOnCompletion(next, event);
    WaitForSingleObject(event, 5000);
    if (with_census) rsf_census_flush();

    const bool rtss = GetModuleHandleW(L"RTSSHooks64.dll") != nullptr;
    std::printf("mode=%s frames=%u rtss_loaded=%d d3d12_errors=%u multi_buffer_904=%u reset_failures=%u "
                "present_failures=%u\n",
                with_census ? "census" : "plain", frames, rtss ? 1 : 0, g_errors.load(), g_multi_buffer.load(),
                reset_failures, present_failures);
    return (g_errors.load() || reset_failures || present_failures) ? 1 : 0;
}
