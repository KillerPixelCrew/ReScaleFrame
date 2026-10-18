// SPDX-License-Identifier: GPL-3.0-only
#include "preview.h"
#include <rescaleframe/native_fg_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <atomic>
#include <limits>

namespace {
using Microsoft::WRL::ComPtr;
HWND parent = nullptr, window = nullptr;
std::atomic<bool> visible{false};
RECT requested{};
uint64_t best_area = UINT64_MAX;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<IDXGISwapChain3> chain;
ComPtr<ID3D12Fence> fence;
struct Slot {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Resource> source;
    uint64_t complete = 0;
};
std::array<Slot, 3> slots;
uint32_t next_slot = 0;
uint64_t complete = 0;
std::atomic<uint64_t> base_frames{0};
LRESULT CALLBACK procedure(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_NCHITTEST) return HTTRANSPARENT;
    if (message == WM_MOUSEACTIVATE) return MA_NOACTIVATE;
    if (message == WM_ERASEBKGND) return 1;
    return DefWindowProcW(hwnd, message, wparam, lparam);
}
BOOL CALLBACK find_parent(HWND candidate, LPARAM)
{
    DWORD process = 0; GetWindowThreadProcessId(candidate, &process);
    if (process != GetCurrentProcessId() || !IsWindowVisible(candidate)) return TRUE;
    RECT bounds{}; if (!GetWindowRect(candidate, &bounds)) return TRUE;
    if (requested.left < bounds.left || requested.top < bounds.top || requested.right > bounds.right || requested.bottom > bounds.bottom) return TRUE;
    const uint64_t area = uint64_t(bounds.right - bounds.left) * uint64_t(bounds.bottom - bounds.top);
    if (area < best_area) { best_area = area; parent = candidate; }
    return TRUE;
}
bool wait(uint64_t value)
{
    if (!value || (fence && fence->GetCompletedValue() >= value)) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
    const bool done = fence && SUCCEEDED(fence->SetEventOnCompletion(value, event)) && WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event); return done;
}
void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}
}
bool rsf_unity_preview_region(int left, int top, int width, int height, bool show)
{
    visible.store(show && width > 0 && height > 0);
    if (!visible.load()) { if (window) ShowWindow(window, SW_HIDE); return true; }
    requested = {left, top, left + width, top + height};
    if (!window) {
        best_area = UINT64_MAX; parent = nullptr; EnumWindows(find_parent, 0);
        if (!parent || GetWindowThreadProcessId(parent, nullptr) != GetCurrentThreadId()) return false;
        WNDCLASSW type{}; type.lpfnWndProc = procedure; type.hInstance = GetModuleHandleW(nullptr); type.lpszClassName = L"ReScaleFrame.Unity.EditorPreview";
        if (!RegisterClassW(&type) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return false;
        window = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, type.lpszClassName, L"", WS_CHILD | WS_CLIPSIBLINGS,
            0, 0, width, height, parent, nullptr, type.hInstance, nullptr);
        if (!window) return false;
    }
    POINT position{left, top}; ScreenToClient(parent, &position);
    return SetWindowPos(window, nullptr, position.x, position.y, width, height, SWP_NOACTIVATE | SWP_NOZORDER) != 0;
}
bool rsf_unity_preview_enabled() { return window && visible.load(); }
bool rsf_unity_preview_owns(void* candidate) { return window && candidate == window; }
void* rsf_unity_preview_parent() { return parent; }
void rsf_unity_preview_hide() { visible.store(false); if (window) ShowWindowAsync(window, SW_HIDE); }
uint64_t rsf_unity_preview_base_frames() { return base_frames.load(); }
bool rsf_unity_preview_prepare(void* owning_device, void* owning_queue, uint32_t width, uint32_t height)
{
    if (!rsf_unity_preview_enabled() || !owning_device || !owning_queue || !width || !height) return false;
    if (!device) {
        device = static_cast<ID3D12Device*>(owning_device); queue = static_cast<ID3D12CommandQueue*>(owning_queue);
        if (FAILED(device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))) return false;
    }
    if (device.Get() != owning_device || queue.Get() != owning_queue) return false;
    if (!chain) {
        ComPtr<IDXGIFactory2> factory; ComPtr<IDXGISwapChain1> created;
        DXGI_SWAP_CHAIN_DESC1 desc{}; desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
        desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
        desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
        if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) || FAILED(factory->CreateSwapChainForHwnd(queue.Get(), window, &desc, nullptr, nullptr, &created)) ||
            FAILED(created.As(&chain))) return false;
    } else {
        DXGI_SWAP_CHAIN_DESC1 desc{}; chain->GetDesc1(&desc);
        if ((desc.Width != width || desc.Height != height) && (!wait(complete) || FAILED(chain->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0)))) return false;
    }
    return true;
}
bool rsf_unity_preview_present(void* texture, const rsf_game_render_pass& source_pass, bool common_state)
{
    if (!rsf_unity_preview_enabled() || !chain || !texture) return false;
    auto* source = static_cast<ID3D12Resource*>(texture);
    const auto desc = source->GetDesc();
    if (desc.Width != source_pass.camera.output_width || desc.Height != source_pass.camera.output_height ||
        (desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM && desc.Format != DXGI_FORMAT_R8G8B8A8_TYPELESS && desc.Format != DXGI_FORMAT_R8G8B8A8_UNORM_SRGB)) return false;
    if (!rsf_unity_preview_prepare(device.Get(), queue.Get(), source_pass.camera.output_width, source_pass.camera.output_height)) return false;
    auto& slot = slots[next_slot]; if (!wait(slot.complete)) return false;
    slot.source.Reset();
    if (!slot.list) {
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&slot.allocator))) ||
            FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, slot.allocator.Get(), nullptr, IID_PPV_ARGS(&slot.list))) || FAILED(slot.list->Close())) return false;
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return false;
    ComPtr<ID3D12Resource> backbuffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer)))) return false;
    const auto source_state = common_state ? D3D12_RESOURCE_STATE_COMMON : D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier(slot.list.Get(), source, source_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
    barrier(slot.list.Get(), backbuffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    slot.list->CopyResource(backbuffer.Get(), source);
    barrier(slot.list.Get(), backbuffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    barrier(slot.list.Get(), source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_state);
    if (FAILED(slot.list->Close())) return false;
    slot.source = source;
    ID3D12CommandList* lists[] = {slot.list.Get()}; queue->ExecuteCommandLists(1, lists);
    slot.complete = ++complete; if (FAILED(queue->Signal(fence.Get(), complete))) return false;
    auto pass = source_pass; pass.swapchain = chain.Get(); pass.role = RSF_GAME_RENDER_WINDOW;
    rsf_fg12_window(&pass);
    const auto result = chain->Present(0, 0);
    if (SUCCEEDED(result)) { ++base_frames; ShowWindowAsync(window, SW_SHOWNOACTIVATE); }
    next_slot = (next_slot + 1) % 3; return SUCCEEDED(result);
}
