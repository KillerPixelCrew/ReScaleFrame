// SPDX-License-Identifier: MIT
#include "preview.h"
#include "../../../games/unity-mono/src/preview_platform.h"
#include <rescaleframe/native_fg_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <array>
#include <utility>

namespace {
using Microsoft::WRL::ComPtr;
ComPtr<ID3D12Device> device;
ComPtr<ID3D12CommandQueue> queue;
ComPtr<IDXGISwapChain3> chain;
ComPtr<ID3D12Fence> fence;
struct Slot {
    ComPtr<ID3D12CommandAllocator> allocator;
    ComPtr<ID3D12GraphicsCommandList> list;
    ComPtr<ID3D12Resource> source;
};
std::array<Slot, 3> slots;
static_assert(DXGI_FORMAT_R8G8B8A8_TYPELESS == 27 && DXGI_FORMAT_R8G8B8A8_UNORM == 28 &&
    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB == 29);

LRESULT CALLBACK procedure(HWND hwnd, UINT message, WPARAM wparam, LPARAM lparam)
{
    if (message == WM_NCHITTEST) return HTTRANSPARENT;
    if (message == WM_MOUSEACTIVATE) return MA_NOACTIVATE;
    if (message == WM_ERASEBKGND) return 1;
    return DefWindowProcW(hwnd, message, wparam, lparam);
}

struct Enumeration {
    void* user;
    rsf_unity_preview_candidate candidate;
};

BOOL CALLBACK find_parent(HWND candidate, LPARAM data)
{
    DWORD process = 0; GetWindowThreadProcessId(candidate, &process);
    if (process != GetCurrentProcessId() || !IsWindowVisible(candidate)) return TRUE;
    RECT bounds{}; if (!GetWindowRect(candidate, &bounds)) return TRUE;
    const rsf_unity_preview_rect rect{bounds.left, bounds.top, bounds.right, bounds.bottom};
    const auto& request = *reinterpret_cast<const Enumeration*>(data);
    request.candidate(request.user, candidate, &rect);
    return TRUE;
}

void enumerate(void* user, rsf_unity_preview_candidate candidate) noexcept
{
    const Enumeration request{user, candidate};
    EnumWindows(find_parent, reinterpret_cast<LPARAM>(&request));
}

void* create_window(void* parent_window, int32_t width, int32_t height) noexcept
{
    const auto parent = static_cast<HWND>(parent_window);
    if (GetWindowThreadProcessId(parent, nullptr) != GetCurrentThreadId()) return nullptr;
    WNDCLASSW type{};
    type.lpfnWndProc = procedure; type.hInstance = GetModuleHandleW(nullptr);
    type.lpszClassName = L"ReScaleFrame.Unity.EditorPreview";
    if (!RegisterClassW(&type) && GetLastError() != ERROR_CLASS_ALREADY_EXISTS) return nullptr;
    return CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, type.lpszClassName, L"",
        WS_CHILD | WS_CLIPSIBLINGS, 0, 0, width, height, parent, nullptr, type.hInstance, nullptr);
}

bool position(void* window, void* parent, int32_t left, int32_t top, int32_t width, int32_t height) noexcept
{
    POINT point{left, top};
    ScreenToClient(static_cast<HWND>(parent), &point);
    return SetWindowPos(static_cast<HWND>(window), nullptr, point.x, point.y, width, height,
        SWP_NOACTIVATE | SWP_NOZORDER) != 0;
}

void show(void* window, bool visible, bool asynchronous) noexcept
{
    const auto operation = visible ? SW_SHOWNOACTIVATE : SW_HIDE;
    if (asynchronous) ShowWindowAsync(static_cast<HWND>(window), operation);
    else ShowWindow(static_cast<HWND>(window), operation);
}

bool bind_gpu(void* owning_device, void* owning_queue) noexcept
{
    auto* next_device = static_cast<ID3D12Device*>(owning_device);
    ComPtr<ID3D12Fence> next_fence;
    if (FAILED(next_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&next_fence))) ||
        !next_fence) return false;
    device = next_device;
    queue = static_cast<ID3D12CommandQueue*>(owning_queue);
    fence = std::move(next_fence);
    return true;
}

bool create_chain(void* window, uint32_t width, uint32_t height) noexcept
{
    ComPtr<IDXGIFactory2> factory; ComPtr<IDXGISwapChain1> created;
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    return SUCCEEDED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) &&
        SUCCEEDED(factory->CreateSwapChainForHwnd(queue.Get(), static_cast<HWND>(window), &desc, nullptr, nullptr, &created)) &&
        SUCCEEDED(created.As(&chain));
}

bool chain_extent(uint32_t* width, uint32_t* height) noexcept
{
    DXGI_SWAP_CHAIN_DESC1 desc{};
    if (FAILED(chain->GetDesc1(&desc))) return false;
    *width = desc.Width;
    *height = desc.Height;
    return true;
}

bool resize_chain(uint32_t width, uint32_t height) noexcept
{
    return SUCCEEDED(chain->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0));
}

void describe(void* texture, rsf_unity_preview_texture* facts) noexcept
{
    const auto desc = static_cast<ID3D12Resource*>(texture)->GetDesc();
    *facts = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format)};
}

bool wait(uint64_t value) noexcept
{
    if (!value || (fence && fence->GetCompletedValue() >= value)) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
    const bool done = fence && SUCCEEDED(fence->SetEventOnCompletion(value, event)) &&
        WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event); return done;
}

void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}

bool record(uint32_t index, void* texture, bool common_state) noexcept
{
    // Resolve the destination before resetting the slot, so a refusal leaves no open list.
    ComPtr<ID3D12Resource> backbuffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer)))) return false;
    auto& slot = slots[index];
    slot.source.Reset();
    if (!slot.list) {
        Slot fresh;
        if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&fresh.allocator))) ||
            FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, fresh.allocator.Get(), nullptr, IID_PPV_ARGS(&fresh.list))) ||
            FAILED(fresh.list->Close())) return false;
        slot = std::move(fresh);
    }
    if (FAILED(slot.allocator->Reset()) || FAILED(slot.list->Reset(slot.allocator.Get(), nullptr))) return false;
    auto* source = static_cast<ID3D12Resource*>(texture);
    const auto source_state = common_state ? D3D12_RESOURCE_STATE_COMMON : D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier(slot.list.Get(), source, source_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
    barrier(slot.list.Get(), backbuffer.Get(), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    slot.list->CopyResource(backbuffer.Get(), source);
    barrier(slot.list.Get(), backbuffer.Get(), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    barrier(slot.list.Get(), source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_state);
    if (FAILED(slot.list->Close())) { slot = {}; return false; }
    slot.source = source;
    ID3D12CommandList* lists[] = {slot.list.Get()}; queue->ExecuteCommandLists(1, lists);
    return true;
}

bool signal(uint64_t value) noexcept
{
    return SUCCEEDED(queue->Signal(fence.Get(), value));
}

bool present(const rsf_game_render_pass* source) noexcept try
{
    auto pass = *source; pass.swapchain = chain.Get(); pass.role = RSF_GAME_RENDER_WINDOW;
    rsf_fg12_window(&pass);
    return SUCCEEDED(chain->Present(0, 0));
}
catch (...) { return false; }

constexpr rsf_unity_preview_leaves leaves{enumerate, create_window, position, show, bind_gpu,
    create_chain, chain_extent, resize_chain, describe, wait, record, signal, present};
}

bool rsf_unity_preview_region(int left, int top, int width, int height, bool show)
{ return rsf_core_unity_preview_region(left, top, width, height, show, &leaves); }
bool rsf_unity_preview_enabled() { return rsf_core_unity_preview_enabled(); }
bool rsf_unity_preview_owns(void* candidate) { return rsf_core_unity_preview_owns(candidate); }
void* rsf_unity_preview_parent() { return rsf_core_unity_preview_parent(); }
void rsf_unity_preview_hide() { rsf_core_unity_preview_hide(&leaves); }
uint64_t rsf_unity_preview_base_frames() { return rsf_core_unity_preview_base_frames(); }
bool rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height)
{ return rsf_core_unity_preview_prepare(device, queue, width, height, &leaves); }
bool rsf_unity_preview_present(void* texture, const rsf_game_render_pass& pass, bool common_state)
{ return rsf_core_unity_preview_present(texture, &pass, common_state, &leaves); }
