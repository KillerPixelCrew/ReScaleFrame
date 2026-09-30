// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/fg_session.h>
#include <rescaleframe/streamline_host.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
using Microsoft::WRL::ComPtr;
void log_message(void*, const char* message) { std::printf("SDK: %s\n", message); }
void CALLBACK debug_message(D3D12_MESSAGE_CATEGORY, D3D12_MESSAGE_SEVERITY severity,
    D3D12_MESSAGE_ID, LPCSTR description, void*)
{
    if (severity <= D3D12_MESSAGE_SEVERITY_WARNING) std::printf("D3D12: %s\n", description);
}
int main(int argc, char** argv)
{
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    // Opt-in SDK/device/window lifecycle fixture. It never tags game inputs or claims FG pixels.
    if (argc != 4) { std::puts("SKIP: supply backend (1/3/4/5), runtime directory, vendor ID"); return 77; }
    const uint32_t backend = static_cast<uint32_t>(std::strtoul(argv[1], nullptr, 0));
    const uint32_t vendor = static_cast<uint32_t>(std::strtoul(argv[3], nullptr, 0));
    const auto* provider = rsf_fg_get_provider(backend); if (!provider) return 1;
    ComPtr<ID3D12Debug> debug;
    if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    ComPtr<IDXGIFactory4> factory; ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory)))) return 1;
    for (UINT i = 0;; ++i) {
        ComPtr<IDXGIAdapter1> candidate;
        if (factory->EnumAdapters1(i, &candidate) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{}; candidate->GetDesc1(&desc);
        if (desc.VendorId == vendor && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { adapter = candidate; break; }
    }
    if (!adapter) { std::puts("SKIP: selected hardware unavailable"); return 77; }
    DXGI_ADAPTER_DESC1 adapter_desc{}; adapter->GetDesc1(&adapter_desc);
    std::printf("device vendor=%04x device=%04x backend=%u\n", adapter_desc.VendorId, adapter_desc.DeviceId, backend);
    ComPtr<ID3D12Device> device; ComPtr<ID3D12CommandQueue> queue;
    rsf_streamline_host* host = nullptr;
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (backend == RSF_FG_BACKEND_DLSS) {
        rsf_streamline_host_setup setup{sizeof(setup), RSF_STREAMLINE_HOST_ABI_VERSION, argv[2], adapter.Get(), 0,
            "ReScaleFrame fixture", "a3ed1f08-3542-4698-b85c-e1a9908e861a", 1, 0, log_message, nullptr, RSF_SL_PROFILE_DLSS_FG, 1};
        auto result = rsf_streamline_host_create(&setup, &host);
        std::printf("Streamline early host result=%d\n", result);
        if (result != 0) return result == RSF_BACKEND_ERROR_NOT_COMPILED || result == RSF_BACKEND_ERROR_NOT_SUPPORTED ? 77 : 1;
        if (rsf_streamline_host_graphics(host, &graphics) != 0) return 1;
    } else {
        if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_11_0, IID_PPV_ARGS(&device)))) return 1;
        D3D12_COMMAND_QUEUE_DESC description{}; description.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
        if (FAILED(device->CreateCommandQueue(&description, IID_PPV_ARGS(&queue)))) return 1;
        graphics.device = device.Get(); graphics.queue = queue.Get(); graphics.native_device = device.Get(); graphics.native_queue = queue.Get();
    }
    ComPtr<ID3D12InfoQueue1> debug_queue;
    DWORD callback_cookie = 0;
    if (SUCCEEDED(static_cast<ID3D12Device*>(graphics.native_device)->QueryInterface(IID_PPV_ARGS(&debug_queue)))) {
        debug_queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_CORRUPTION, FALSE);
        debug_queue->SetBreakOnSeverity(D3D12_MESSAGE_SEVERITY_ERROR, FALSE);
        debug_queue->RegisterMessageCallback(debug_message, D3D12_MESSAGE_CALLBACK_FLAG_NONE, nullptr, &callback_cookie);
    }
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RSFFGFixture";
    if (!RegisterClassW(&wc)) return 1;
    HWND window = CreateWindowW(wc.lpszClassName, L"FG lifecycle fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 1920, 1080, nullptr, nullptr, wc.hInstance, nullptr);
    if (!window) return 1;
    rsf_generation_setup setup{}; setup.struct_size = sizeof(setup); setup.abi_version = RSF_FG_ABI_VERSION;
    setup.chain.struct_size = sizeof(setup.chain); setup.chain.abi_version = RSF_BACKEND_ABI_VERSION;
    setup.chain.d3d12_device = graphics.device; setup.chain.d3d12_queue = graphics.queue;
    setup.chain.hwnd = window; setup.chain.width = 1920; setup.chain.height = 1080;
    setup.chain.format = DXGI_FORMAT_R8G8B8A8_UNORM; setup.chain.buffer_count = 3;
    setup.chain.max_generated_frames = 5; setup.chain.ui_mode = RSF_UI_MODE_NONE;
    setup.feature_major = backend; setup.runtime_directory_utf8 = argv[2]; setup.view_space_to_meters = 1;
    setup.depth_inverted = 1; setup.depth_infinite = 1; setup.streamline_host = host;
    setup.chain.log = log_message;
    void* context = nullptr; void* chain = nullptr;
    auto result = provider->create(&setup, &context, &chain);
    std::printf("provider create result=%d chain=%u\n", result, chain != nullptr);
    int exit_code = 0;
    if (result != 0) exit_code = result == RSF_BACKEND_ERROR_NOT_SUPPORTED || result == RSF_BACKEND_ERROR_NOT_COMPILED ? 77 : 1;
    else {
        rsf_fg_status state{}; state.struct_size = sizeof(state);
        result = provider->status(context, &state);
        std::printf("status result=%d supported=%u maxGenerated=%u dynamic=%u minDimension=%u version=%s\n",
            result, state.supported, state.max_generated_frames, state.dynamic_supported, state.min_dimension, state.version_name);
        rsf_fg_options options{sizeof(options), RSF_FG_ABI_VERSION, RSF_FG_OFF, 1, 0, RSF_REFLEX_OFF, 0};
        if (result != 0 || !chain || provider->configure(context, &options) != 0) exit_code = 1;
        // Only disabled presents: no fabricated depth, motion, camera, input or simulation.
        auto* swap = static_cast<IDXGISwapChain4*>(chain);
        if (!exit_code) {
            ComPtr<ID3D12Resource> backbuffer;
            if (FAILED(swap->GetBuffer(swap->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer)))) return 1;
            const auto presented = swap->Present(0, 0);
            std::printf("disabled Present HRESULT=%08lx\n", presented);
            if (FAILED(presented)) exit_code = 1;
            if (provider->after_present(context) != 0) exit_code = 1;
        }
        ComPtr<ID3D12Fence> fence;
        auto* native_device = static_cast<ID3D12Device*>(graphics.native_device);
        auto* native_queue = static_cast<ID3D12CommandQueue*>(graphics.native_queue);
        HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
        if (!event || FAILED(native_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence))) ||
            FAILED(native_queue->Signal(fence.Get(), 1)) || FAILED(fence->SetEventOnCompletion(1, event)) ||
            WaitForSingleObject(event, 15000) != WAIT_OBJECT_0) {
            std::printf("FAIL: GPU did not retire, removed=%08lx; context retained until process exits\n", native_device->GetDeviceRemovedReason());
            ComPtr<ID3D12InfoQueue> info;
            if (SUCCEEDED(native_device->QueryInterface(IID_PPV_ARGS(&info)))) {
                const auto count = info->GetNumStoredMessages();
                for (uint64_t i = count > 12 ? count - 12 : 0; i < count; ++i) {
                    SIZE_T length = 0; info->GetMessage(i, nullptr, &length);
                    auto* data = static_cast<D3D12_MESSAGE*>(std::malloc(length));
                    if (data) { if (SUCCEEDED(info->GetMessage(i, data, &length))) std::printf("D3D12: %s\n", data->pDescription); std::free(data); }
                }
            }
            return 1;
        }
        CloseHandle(event);
        rsf_fg_retirement retirement{}; retirement.struct_size = sizeof(retirement);
        if (provider->retirement(context, &retirement) != 0) return 1;
        if (retirement.fence && static_cast<ID3D12Fence*>(retirement.fence)->GetCompletedValue() < retirement.value) return 1;
        provider->destroy(context); context = nullptr;
    }
    DestroyWindow(window); UnregisterClassW(wc.lpszClassName, wc.hInstance);
    if (debug_queue) debug_queue->UnregisterMessageCallback(callback_cookie);
    debug_queue.Reset();
    if (host && rsf_streamline_host_destroy(host) != 0) exit_code = 1;
    std::printf("lifecycle result=%d; generated pixels and latency NOT TESTED\n", exit_code);
    return exit_code;
}
