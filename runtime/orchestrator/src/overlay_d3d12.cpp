// SPDX-License-Identifier: MIT
#include "overlay_wrapped12_platform.h"
#include <rescaleframe/backend.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include <windows.h>
#include <d3d11on12.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
namespace {
using Microsoft::WRL::ComPtr;
using CreateOn12 = decltype(&D3D11On12CreateDevice);
int32_t create_wrapped_device(void* device, void* queue, rsf_overlay11on12_device* out) noexcept try {
    const auto create_on12 = reinterpret_cast<CreateOn12>(rsf_core_overlay11on12_create_proc());
    if (!create_on12) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    IUnknown* queues[] = {static_cast<ID3D12CommandQueue*>(queue)};
    ComPtr<ID3D11Device> device11;
    ComPtr<ID3D11DeviceContext> context11;
    ComPtr<ID3D11On12Device> interop;
    if (FAILED(create_on12(static_cast<ID3D12Device*>(device), D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        nullptr, 0, queues, 1, 0, &device11, &context11, nullptr)) || FAILED(device11.As(&interop)))
        return RSF_BACKEND_ERROR_INIT_FAILED;
    *out = {device11.Detach(), context11.Detach(), interop.Detach()};
    return RSF_BACKEND_OK;
} catch (...) { return RSF_BACKEND_ERROR_INIT_FAILED; }

uint32_t window(void* chain, void** out) {
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc))) return 0;
    *out = desc.OutputWindow;
    return 1;
}

uint32_t wrap_target(void* device, void* interop, ComPtr<ID3D12Resource> back, rsf_overlay11on12_target* out,
    D3D12_RESOURCE_STATES state) {
    D3D11_RESOURCE_FLAGS flags{}; flags.BindFlags = D3D11_BIND_RENDER_TARGET;
    ComPtr<ID3D11Resource> wrapped;
    if (FAILED(static_cast<ID3D11On12Device*>(interop)->CreateWrappedResource(back.Get(), &flags,
        state, state, IID_PPV_ARGS(&wrapped)))) return 0;
    ComPtr<ID3D11RenderTargetView> view;
    if (FAILED(static_cast<ID3D11Device*>(device)->CreateRenderTargetView(wrapped.Get(), nullptr, &view))) return 0;
    const auto desc = back->GetDesc();
    if (!desc.Width || desc.Width > UINT32_MAX || !desc.Height) return 0;
    *out = {back.Detach(), wrapped.Detach(), view.Detach(), static_cast<uint32_t>(desc.Width), desc.Height};
    return 1;
}

uint32_t target(void* device, void* interop, void* chain, rsf_overlay11on12_target* out) {
    ComPtr<IDXGISwapChain3> chain3;
    ComPtr<ID3D12Resource> back;
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->QueryInterface(IID_PPV_ARGS(&chain3))) ||
        FAILED(chain3->GetBuffer(chain3->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&back)))) return 0;
    return wrap_target(device, interop, back, out, D3D12_RESOURCE_STATE_RENDER_TARGET);
}

uint32_t target_present(void* device, void* interop, void* backbuffer, rsf_overlay11on12_target* out) {
    ComPtr<ID3D12Resource> back(static_cast<ID3D12Resource*>(backbuffer));
    return wrap_target(device, interop, back, out, D3D12_RESOURCE_STATE_PRESENT);
}

void acquire(void* interop, void* wrapped) {
    ID3D11Resource* resources[] = {static_cast<ID3D11Resource*>(wrapped)};
    static_cast<ID3D11On12Device*>(interop)->AcquireWrappedResources(resources, 1);
}

void release(void* interop, void* wrapped) {
    ID3D11Resource* resources[] = {static_cast<ID3D11Resource*>(wrapped)};
    static_cast<ID3D11On12Device*>(interop)->ReleaseWrappedResources(resources, 1);
}

int start(void* device, void* hwnd, rsf_overlay12_log log, void* user) noexcept try {
    return rsf_overlay_host_start_device(device, hwnd, log, user);
} catch (...) { return 0; }

int draw(void* context, void* view, uint32_t width, uint32_t height,
    const rsf_overlay_stats* stats, rsf_overlay_intent* intent, uint32_t visible, float hint) noexcept try {
    return rsf_overlay_host_draw_admitted_target(context, view, width, height, stats, intent, visible, hint);
} catch (...) { return 0; }

constexpr rsf_overlay11on12_platform platform{
    create_wrapped_device, window, start, rsf_overlay_host_admit_target, target, acquire, draw, release,
    [](void* context) { static_cast<ID3D11DeviceContext*>(context)->Flush(); },
    rsf_overlay_host_stop_device,
};
constexpr auto present_platform = [] {
    auto result = platform;
    result.target = target_present;
    return result;
}();
}

int rsf_overlay_d3d12_frame(void* device, void* queue, void* chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent, void (*log)(void*,const char*), void* user) {
    return rsf_core_overlay11on12_frame(&platform, device, queue, chain, nullptr, stats, intent, log, user);
}
int rsf_overlay_d3d12_frame_for_window(void* device, void* queue, void* backbuffer, void* input_window,
    const rsf_overlay_stats* stats, rsf_overlay_intent* intent, void (*log)(void*, const char*), void* user) {
    if (!IsWindow(static_cast<HWND>(input_window))) return -1;
    return rsf_core_overlay11on12_frame(&present_platform, device, queue, backbuffer, input_window, stats, intent, log, user);
}
int rsf_overlay_d3d12_present(void* device, void* queue, void* chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent, void (*log)(void*,const char*), void* user) {
    return rsf_overlay_native12_present(device, queue, chain, stats, intent, log, user);
}
void rsf_overlay_d3d12_stop() {
    rsf_core_overlay11on12_stop(&platform);
}
