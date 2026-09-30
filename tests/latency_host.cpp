// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/streamline_host.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstdlib>
using Microsoft::WRL::ComPtr;
void log_message(void*, const char* message) { std::printf("SDK: %s\n", message); }
bool drain(ID3D12CommandQueue* queue, ID3D12Fence* fence, uint64_t value)
{
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
    const bool result = SUCCEEDED(queue->Signal(fence, value)) && SUCCEEDED(fence->SetEventOnCompletion(value, event)) &&
        WaitForSingleObject(event, 15000) == WAIT_OBJECT_0;
    CloseHandle(event); return result;
}
int main(int argc, char** argv)
{
    if (argc != 4) { std::puts("SKIP: supply runtime directory, vendor ID and profile (1 Reflex / 2 PCL)"); return 77; }
    const uint32_t vendor = static_cast<uint32_t>(std::strtoul(argv[2], nullptr, 0));
    const uint32_t profile = static_cast<uint32_t>(std::strtoul(argv[3], nullptr, 0));
    ComPtr<IDXGIFactory4> discovery; ComPtr<IDXGIAdapter1> adapter;
    if (FAILED(CreateDXGIFactory2(0, IID_PPV_ARGS(&discovery)))) return 1;
    for (UINT index = 0;; ++index) {
        ComPtr<IDXGIAdapter1> candidate;
        if (discovery->EnumAdapters1(index, &candidate) == DXGI_ERROR_NOT_FOUND) break;
        DXGI_ADAPTER_DESC1 desc{}; candidate->GetDesc1(&desc);
        if (desc.VendorId == vendor && !(desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE)) { adapter = candidate; break; }
    }
    if (!adapter) return 77;
    DXGI_ADAPTER_DESC1 adapter_desc{}; adapter->GetDesc1(&adapter_desc);
    std::printf("adapter vendor=%04x device=%04x\n", adapter_desc.VendorId, adapter_desc.DeviceId);
    rsf_streamline_host_setup setup{sizeof(setup), RSF_STREAMLINE_HOST_ABI_VERSION, argv[1], adapter.Get(), 0,
        "ReScaleFrame latency fixture", "a3ed1f08-3542-4698-b85c-e1a9908e861a", 1, 0, log_message, nullptr, profile, 0};
    rsf_streamline_host* host = nullptr;
    auto result = rsf_streamline_host_create(&setup, &host); std::printf("host result=%d profile=%u\n", result, profile);
    if (result != 0) return result == RSF_BACKEND_ERROR_NOT_COMPILED ? 77 : 1;
    rsf_streamline_graphics graphics{}; graphics.struct_size = sizeof(graphics);
    if (rsf_streamline_host_graphics(host, &graphics) != 0) return 1;
    auto* device = static_cast<ID3D12Device*>(graphics.device); auto* queue = static_cast<ID3D12CommandQueue*>(graphics.queue);
    WNDCLASSW wc{}; wc.lpfnWndProc = DefWindowProcW; wc.hInstance = GetModuleHandleW(nullptr); wc.lpszClassName = L"RSFLatencyFixture";
    if (!RegisterClassW(&wc)) return 1;
    HWND window = CreateWindowW(wc.lpszClassName, L"Latency service fixture", WS_OVERLAPPEDWINDOW,
        0, 0, 256, 256, nullptr, nullptr, wc.hInstance, nullptr); if (!window) return 1;
    DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = 256; desc.Height = 256; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    ComPtr<IDXGISwapChain1> first;
    if (FAILED(static_cast<IDXGIFactory2*>(graphics.factory)->CreateSwapChainForHwnd(queue, window, &desc, nullptr, nullptr, &first))) return 1;
    void* raw = first.Detach();
    if (rsf_streamline_host_upgrade_chain(host, &raw) != 0) return 1;
    ComPtr<IDXGISwapChain4> swap;
    auto* upgraded = static_cast<IDXGISwapChain1*>(raw);
    const auto chain_result = upgraded->QueryInterface(IID_PPV_ARGS(&swap)); upgraded->Release(); if (FAILED(chain_result)) return 1;
    ComPtr<ID3D12CommandAllocator> allocator; ComPtr<ID3D12GraphicsCommandList> list; ComPtr<ID3D12Fence> fence;
    if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator))) ||
        FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, allocator.Get(), nullptr, IID_PPV_ARGS(&list))) ||
        FAILED(list->Close()) || FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return 1;
    D3D12_DESCRIPTOR_HEAP_DESC heap_desc{}; heap_desc.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV; heap_desc.NumDescriptors = 1;
    ComPtr<ID3D12DescriptorHeap> heap; if (FAILED(device->CreateDescriptorHeap(&heap_desc, IID_PPV_ARGS(&heap)))) return 1;
    const auto rtv = heap->GetCPUDescriptorHandleForHeapStart();
    float simulation = 0;
    for (uint32_t frame = 0; frame < 12; ++frame) {
        if (profile == RSF_SL_PROFILE_REFLEX && frame % 4 == 0) {
            result = rsf_streamline_host_reflex(host, frame / 4, 0);
            if (result != 0) { std::printf("Reflex mode %u refused=%d\n", frame / 4, result); return result == RSF_BACKEND_ERROR_NOT_SUPPORTED ? 77 : 1; }
        }
        const uint64_t id = UINT64_C(0x100000000) + frame + 1;
        if (rsf_streamline_host_begin(host, id) != 0) return 1;
        const bool pressed = (GetAsyncKeyState(VK_SPACE) & 0x8000) != 0;
        if (rsf_streamline_host_marker(host, id, RSF_LATENCY_INPUT_SAMPLE, 0) != 0 ||
            rsf_streamline_host_marker(host, id, RSF_LATENCY_SIMULATION_START, 0) != 0) return 1;
        simulation += pressed ? 0.02f : 0.01f;
        if (rsf_streamline_host_marker(host, id, RSF_LATENCY_SIMULATION_END, 0) != 0 ||
            rsf_streamline_host_marker(host, id, RSF_LATENCY_RENDER_SUBMIT_START, 0) != 0) return 1;
        ComPtr<ID3D12Resource> backbuffer;
        if (FAILED(swap->GetBuffer(swap->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer))) ||
            FAILED(allocator->Reset()) || FAILED(list->Reset(allocator.Get(), nullptr))) return 1;
        device->CreateRenderTargetView(backbuffer.Get(), nullptr, rtv);
        D3D12_RESOURCE_BARRIER barrier{}; barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition = {backbuffer.Get(), D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET};
        list->ResourceBarrier(1, &barrier);
        const float color[] = {simulation, 0.1f, 0.2f, 1}; list->ClearRenderTargetView(rtv, color, 0, nullptr);
        barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET; barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_PRESENT;
        list->ResourceBarrier(1, &barrier); if (FAILED(list->Close())) return 1;
        ID3D12CommandList* commands[] = {list.Get()}; queue->ExecuteCommandLists(1, commands);
        if (rsf_streamline_host_marker(host, id, RSF_LATENCY_RENDER_SUBMIT_END, 0) != 0 ||
            rsf_streamline_host_marker(host, id, RSF_LATENCY_PRESENT_START, 0) != 0 || FAILED(swap->Present(0, 0)) ||
            rsf_streamline_host_marker(host, id, RSF_LATENCY_PRESENT_END, 0) != 0 || !drain(queue, fence.Get(), frame + 1)) return 1;
    }
    rsf_streamline_latency_status state{}; state.struct_size = sizeof(state);
    if (rsf_streamline_host_latency_status(host, &state) != 0) return 1;
    std::printf("profile=%u sleepCalls=%llu markerCalls=%llu lastID=%llu ReflexAvailable=%u latencyReport=%u\n",
        state.profile, static_cast<unsigned long long>(state.sleep_calls), static_cast<unsigned long long>(state.marker_calls),
        static_cast<unsigned long long>(state.last_begin_id), state.low_latency_available, state.latency_report_available);
    if (state.sleep_calls != (profile == RSF_SL_PROFILE_PCL ? 0u : 12u) || state.marker_calls != 72) return 1;
    heap.Reset(); fence.Reset(); list.Reset(); allocator.Reset(); swap.Reset();
    DestroyWindow(window); UnregisterClassW(wc.lpszClassName, wc.hInstance);
    if (rsf_streamline_host_destroy(host) != 0) return 1;
    std::puts("Independent latency service API counts, 64-bit lineage and plain Present passed; ETW/physical latency NOT TESTED");
}
