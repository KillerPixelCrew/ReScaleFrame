// SPDX-License-Identifier: MIT
// Typed Win32, D3D12 and DXGI leaves of the Unity Editor preview. Rust
// (games/unity-mono/src/preview.rs) owns the window record, the device, queue, fence and swapchain
// references, each ring slot's allocator, list and source reference, the submission ring and the
// checked release. No leaf keeps a reference past its return.
#include "preview.h"
#include "../../../games/unity-mono/src/preview_platform.h"
#include <rescaleframe/native_fg_d3d12.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>

namespace {
using Microsoft::WRL::ComPtr;
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

// Only the fence creation can fail, so the three references transfer together or not at all.
bool bind_gpu(void* owning_device, void* owning_queue, rsf_unity_preview_gpu* output) noexcept
{
    auto* next_device = static_cast<ID3D12Device*>(owning_device);
    ComPtr<ID3D12Fence> next_fence;
    if (FAILED(next_device->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&next_fence))) ||
        !next_fence) return false;
    next_device->AddRef(); output->device = next_device;
    static_cast<ID3D12CommandQueue*>(owning_queue)->AddRef(); output->queue = owning_queue;
    output->fence = next_fence.Detach();
    return true;
}

bool create_chain(const rsf_unity_preview_gpu* gpu, void* window, uint32_t width, uint32_t height, void** output) noexcept
{
    ComPtr<IDXGIFactory2> factory; ComPtr<IDXGISwapChain1> created; ComPtr<IDXGISwapChain3> chain;
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = width; desc.Height = height; desc.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
    desc.SampleDesc.Count = 1; desc.BufferCount = 3; desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->CreateSwapChainForHwnd(static_cast<ID3D12CommandQueue*>(gpu->queue),
            static_cast<HWND>(window), &desc, nullptr, nullptr, &created)) ||
        FAILED(created.As(&chain))) return false;
    *output = chain.Detach();
    return true;
}

bool chain_extent(void* chain, uint32_t* width, uint32_t* height) noexcept
{
    DXGI_SWAP_CHAIN_DESC1 desc{};
    if (FAILED(static_cast<IDXGISwapChain3*>(chain)->GetDesc1(&desc))) return false;
    *width = desc.Width;
    *height = desc.Height;
    return true;
}

bool resize_chain(void* chain, uint32_t width, uint32_t height) noexcept
{
    return SUCCEEDED(static_cast<IDXGISwapChain3*>(chain)->ResizeBuffers(3, width, height, DXGI_FORMAT_R8G8B8A8_UNORM, 0));
}

void describe(void* texture, rsf_unity_preview_texture* facts) noexcept
{
    const auto desc = static_cast<ID3D12Resource*>(texture)->GetDesc();
    *facts = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format)};
}

uint64_t completed(void* fence) noexcept
{
    return static_cast<ID3D12Fence*>(fence)->GetCompletedValue();
}

bool wait(void* fence_handle, uint64_t value) noexcept
{
    auto* fence = static_cast<ID3D12Fence*>(fence_handle);
    if (!value || (fence && fence->GetCompletedValue() >= value)) return true;
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr); if (!event) return false;
    const bool done = fence && SUCCEEDED(fence->SetEventOnCompletion(value, event)) &&
        WaitForSingleObject(event, 10000) == WAIT_OBJECT_0;
    CloseHandle(event); return done;
}

bool acquire(void* chain_handle, void** output) noexcept
{
    auto* chain = static_cast<IDXGISwapChain3*>(chain_handle);
    ComPtr<ID3D12Resource> backbuffer;
    if (FAILED(chain->GetBuffer(chain->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&backbuffer)))) return false;
    *output = backbuffer.Detach();
    return true;
}

bool create_slot(void* device_handle, rsf_unity_preview_slot* output) noexcept
{
    auto* device = static_cast<ID3D12Device*>(device_handle);
    ComPtr<ID3D12CommandAllocator> allocator;
    if (FAILED(device->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&allocator)))) return false;
    output->allocator = allocator.Detach();
    ComPtr<ID3D12GraphicsCommandList> list;
    if (FAILED(device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
            static_cast<ID3D12CommandAllocator*>(output->allocator), nullptr, IID_PPV_ARGS(&list)))) return false;
    output->list = list.Detach();
    return SUCCEEDED(static_cast<ID3D12GraphicsCommandList*>(output->list)->Close());
}

void barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* resource, D3D12_RESOURCE_STATES from, D3D12_RESOURCE_STATES to)
{
    D3D12_RESOURCE_BARRIER b{}; b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    b.Transition = {resource, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, from, to}; list->ResourceBarrier(1, &b);
}

uint32_t record(const rsf_unity_preview_slot* slot, void* destination, void* texture, bool common_state) noexcept
{
    auto* allocator = static_cast<ID3D12CommandAllocator*>(slot->allocator);
    auto* list = static_cast<ID3D12GraphicsCommandList*>(slot->list);
    if (FAILED(allocator->Reset()) || FAILED(list->Reset(allocator, nullptr))) return RSF_UNITY_PREVIEW_RECORD_REFUSED;
    auto* source = static_cast<ID3D12Resource*>(texture);
    auto* backbuffer = static_cast<ID3D12Resource*>(destination);
    const auto source_state = common_state ? D3D12_RESOURCE_STATE_COMMON : D3D12_RESOURCE_STATE_RENDER_TARGET;
    barrier(list, source, source_state, D3D12_RESOURCE_STATE_COPY_SOURCE);
    barrier(list, backbuffer, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_DEST);
    list->CopyResource(backbuffer, source);
    barrier(list, backbuffer, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_PRESENT);
    barrier(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, source_state);
    return SUCCEEDED(list->Close()) ? RSF_UNITY_PREVIEW_RECORD_CLOSED : RSF_UNITY_PREVIEW_RECORD_BROKEN;
}

void execute(void* queue, void* list) noexcept
{
    ID3D12CommandList* lists[] = {static_cast<ID3D12GraphicsCommandList*>(list)};
    static_cast<ID3D12CommandQueue*>(queue)->ExecuteCommandLists(1, lists);
}

bool signal(void* queue, void* fence, uint64_t value) noexcept
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value));
}

bool present(void* chain, const rsf_game_render_pass* source) noexcept try
{
    auto pass = *source; pass.swapchain = chain; pass.role = RSF_GAME_RENDER_WINDOW;
    rsf_fg12_window(&pass);
    return SUCCEEDED(static_cast<IDXGISwapChain3*>(chain)->Present(0, 0));
}
catch (...) { return false; }

void retain(void* object) noexcept { static_cast<IUnknown*>(object)->AddRef(); }
void release(void* object) noexcept { if (object) static_cast<IUnknown*>(object)->Release(); }

constexpr rsf_unity_preview_leaves leaves{enumerate, create_window, position, show, bind_gpu,
    create_chain, chain_extent, resize_chain, describe, completed, wait, acquire, create_slot, record,
    execute, signal, present, retain, release};
}

bool rsf_unity_preview_region(int left, int top, int width, int height, bool show)
{ return rsf_core_unity_preview_region(left, top, width, height, show, &leaves); }
bool rsf_unity_preview_enabled() { return rsf_core_unity_preview_enabled(); }
bool rsf_unity_preview_owns(void* candidate) { return rsf_core_unity_preview_owns(candidate); }
void* rsf_unity_preview_parent() { return rsf_core_unity_preview_parent(); }
void rsf_unity_preview_hide() { rsf_core_unity_preview_hide(&leaves); }
bool rsf_unity_preview_drained() noexcept { return rsf_core_unity_preview_drained(&leaves); }
// The package commits only after rsf_unity_preview_drained passed with every bridge route closed,
// so the proof still holds here. A refusal keeps every reference, as a faulted preview does.
void rsf_unity_preview_release() noexcept { rsf_core_unity_preview_release(&leaves); }
uint64_t rsf_unity_preview_base_frames() { return rsf_core_unity_preview_base_frames(); }
bool rsf_unity_preview_prepare(void* device, void* queue, uint32_t width, uint32_t height)
{ return rsf_core_unity_preview_prepare(device, queue, width, height, &leaves); }
bool rsf_unity_preview_present(void* texture, const rsf_game_render_pass& pass, bool common_state)
{ return rsf_core_unity_preview_present(texture, &pass, common_state, &leaves); }
