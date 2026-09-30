// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_session.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
using Microsoft::WRL::ComPtr;
struct Host {
    ComPtr<IDXGIFactory4> factory;
    ComPtr<ID3D12Device> device;
    ComPtr<ID3D12CommandQueue> queue;
    ComPtr<ID3D12Fence> fence;
    ComPtr<IDXGISwapChain1> plain;
    IDXGISwapChain4* borrowed = nullptr;
    HWND window = nullptr;
    uint64_t sequence = 0;
};
rsf_backend_result quiesce(void* pointer)
{
    auto& host = *static_cast<Host*>(pointer);
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return RSF_BACKEND_ERROR_FEATURE_FAILED;
    const uint64_t value = ++host.sequence;
    const bool ok = SUCCEEDED(host.queue->Signal(host.fence.Get(), value)) &&
        SUCCEEDED(host.fence->SetEventOnCompletion(value, event)) && WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
    CloseHandle(event); return ok ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_FEATURE_FAILED;
}
void resume(void*) {}
rsf_backend_result release(void* pointer)
{
    auto& host = *static_cast<Host*>(pointer); host.borrowed = nullptr; host.plain.Reset(); return RSF_BACKEND_OK;
}
rsf_backend_result adopt(void* pointer, void* chain)
{
    auto& host = *static_cast<Host*>(pointer);
    if (!chain || host.plain || host.borrowed) return RSF_BACKEND_ERROR_NOT_READY;
    host.borrowed = static_cast<IDXGISwapChain4*>(chain); return RSF_BACKEND_OK;
}
rsf_backend_result plain(void* pointer)
{
    auto& host = *static_cast<Host*>(pointer);
    DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = 1920; desc.Height = 1080;
    desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM; desc.SampleDesc.Count = 1;
    desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    return SUCCEEDED(host.factory->CreateSwapChainForHwnd(host.queue.Get(), host.window, &desc,
        nullptr, nullptr, &host.plain)) ? RSF_BACKEND_OK : RSF_BACKEND_ERROR_INIT_FAILED;
}
bool present(Host& host)
{
    if (!host.borrowed) return false;
    ComPtr<ID3D12Resource> backbuffer;
    return SUCCEEDED(host.borrowed->GetBuffer(host.borrowed->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer))) &&
        SUCCEEDED(host.borrowed->Present(0, 0));
}
int main(int argc, char** argv)
{
    if (argc != 4) { std::puts("SKIP: supply FFX directory, XeFG directory and adapter vendor ID"); return 77; }
    Host host;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&host.factory)))) return 1;
    const uint32_t vendor = static_cast<uint32_t>(std::strtoul(argv[3], nullptr, 0));
    ComPtr<IDXGIAdapter1> adapter;
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> candidate;
        if (host.factory->EnumAdapters1(i, &candidate) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{}; candidate->GetDesc1(&desc);
        if (desc.VendorId == vendor && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { adapter = candidate; break; }
    }
    if (!adapter) return 77;
    if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&host.device)))) return 1;
    D3D12_COMMAND_QUEUE_DESC queue{}; queue.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    if (FAILED(host.device->CreateCommandQueue(&queue, IID_PPV_ARGS(&host.queue))) ||
        FAILED(host.device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&host.fence)))) return 1;
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RSFFGSwitching";
    if (!RegisterClassW(&wc)) return 1;
    host.window = CreateWindowW(wc.lpszClassName, L"FG owner switching fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 1920, 1080, nullptr, nullptr, wc.hInstance, nullptr);
    if (!host.window) return 1;
    rsf_fg_host callbacks{sizeof(callbacks), RSF_FG_ABI_VERSION, &host, quiesce, resume, release, adopt, plain};
    rsf_fg_session* session = nullptr;
    if (rsf_fg_session_create(&callbacks, &session) != 0) return 1;
    rsf_generation_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FG_ABI_VERSION;
    setup.chain.struct_size = sizeof(setup.chain); setup.chain.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.chain.d3d12_device = host.device.Get(); setup.chain.d3d12_queue = host.queue.Get(); setup.chain.hwnd = host.window;
    setup.chain.width = 1920; setup.chain.height = 1080; setup.chain.buffer_count = 3;
    setup.chain.format = DXGI_FORMAT_R8G8B8A8_UNORM; setup.chain.max_generated_frames = 1;
    setup.chain.ui_mode = RSF_UI_MODE_NONE;
    setup.depth_inverted = 1; setup.depth_infinite = 1; setup.view_space_to_meters = 1;
    rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_OFF, 1, 0, RSF_REFLEX_OFF, 0};
    const uint32_t backends[] = {RSF_FG_BACKEND_FSR3, RSF_FG_BACKEND_XESS, RSF_FG_BACKEND_FSR3};
    for (const uint32_t backend : backends) {
        setup.feature_major = backend; setup.runtime_directory_utf8 = backend == RSF_FG_BACKEND_XESS ? argv[2] : argv[1];
        const auto result = rsf_fg_session_select(session, rsf_fg_get_provider(backend), &setup, &options);
        std::printf("select backend=%u result=%d\n", backend, result);
        if (result == RSF_BACKEND_ERROR_NOT_SUPPORTED || result == RSF_BACKEND_ERROR_NOT_COMPILED) return 77;
        if (result != 0 || !present(host)) return 1;
        // This is an Off-only ownership check, not a source-frame latency/tagging experiment.
    }
    if (rsf_fg_session_select(session, nullptr, nullptr, nullptr) != 0 || !host.plain) return 1;
    if (rsf_fg_session_destroy(session) != 0) return 1;
    host.plain.Reset(); DestroyWindow(host.window); UnregisterClassW(wc.lpszClassName, wc.hInstance);
    std::puts("FSR3 -> XeFG -> FSR3 -> plain ownership, disabled Present and teardown passed; enabled FG NOT TESTED");
}
