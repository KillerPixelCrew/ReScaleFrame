// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/overlay_d3d12.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include <windows.h>
#include <d3d11on12.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <mutex>
namespace {
using Microsoft::WRL::ComPtr;
ComPtr<ID3D11Device> device11;
ComPtr<ID3D11DeviceContext> context11;
ComPtr<ID3D11On12Device> interop;
ComPtr<ID3D12Device> owner;
std::mutex drawing;
bool ready = false;
// A refused start is final for this attach. overlay_host reloads the panel library on every
// attempt, so retrying per frame would leak a module reference and log each frame.
bool failed = false;
ULONGLONG ready_tick = 0;
// How long overlay_host shows its startup hint after the panel starts.
constexpr ULONGLONG kStartupHintMs = 8000;
int refuse(void (*log)(void*,const char*), void* user, const char* message)
{
    failed = true;
    if (log) log(user, message);
    return 0;
}
}
int rsf_overlay_d3d12_frame(void* device, void* native_queue, void* native_chain, const rsf_overlay_stats* stats,
    rsf_overlay_intent* intent, void (*log)(void*,const char*), void* user)
{
    std::unique_lock<std::mutex> lock(drawing, std::try_to_lock);
    if (!lock.owns_lock()) return 0;
    if (failed || !device || !native_queue || !native_chain || !stats) return 0;
    auto* chain = static_cast<IDXGISwapChain*>(native_chain);
    if (!device11) {
        IUnknown* queues[] = {static_cast<ID3D12CommandQueue*>(native_queue)};
        if (FAILED(D3D11On12CreateDevice(static_cast<ID3D12Device*>(device), D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            nullptr, 0, queues, 1, 0, &device11, &context11, nullptr)) || FAILED(device11.As(&interop))) {
            interop.Reset(); context11.Reset(); device11.Reset();
            return refuse(log, user, "Shared overlay unavailable: the D3D11-on-12 device could not be created.");
        }
        owner = static_cast<ID3D12Device*>(device);
    }
    if (owner.Get() != device) return 0;
    if (!ready) {
        DXGI_SWAP_CHAIN_DESC desc{};
        if (FAILED(chain->GetDesc(&desc))) return 0;
        if (!rsf_overlay_host_start_device(device11.Get(), desc.OutputWindow, log, user))
            return refuse(log, user, "Shared overlay did not start; it will not be retried.");
        ready = true; ready_tick = GetTickCount64();
        if (log) log(user, "Shared overlay attached to Unity D3D12 queue and window; Insert opens presets.");
    }
    // Hidden, no HUD and past the startup hint: nothing will be drawn, so skip the wrap, view,
    // acquire/release and flush.
    if (!rsf_overlay_host_visible() && !stats->show_performance_hud && GetTickCount64() - ready_tick >= kStartupHintMs) return 0;
    ComPtr<IDXGISwapChain3> chain3;
    if (FAILED(chain->QueryInterface(IID_PPV_ARGS(&chain3)))) return 0;
    ComPtr<ID3D12Resource> back;
    if (FAILED(chain3->GetBuffer(chain3->GetCurrentBackBufferIndex(), IID_PPV_ARGS(&back)))) return 0;
    D3D11_RESOURCE_FLAGS flags{}; flags.BindFlags = D3D11_BIND_RENDER_TARGET;
    ComPtr<ID3D11Resource> wrapped;
    if (FAILED(interop->CreateWrappedResource(back.Get(), &flags, D3D12_RESOURCE_STATE_RENDER_TARGET,
        D3D12_RESOURCE_STATE_RENDER_TARGET, IID_PPV_ARGS(&wrapped)))) return 0;
    ComPtr<ID3D11RenderTargetView> target;
    if (FAILED(device11->CreateRenderTargetView(wrapped.Get(), nullptr, &target))) return 0;
    ID3D11Resource* acquired[] = {wrapped.Get()}; interop->AcquireWrappedResources(acquired, 1);
    const auto desc = back->GetDesc();
    const int drawn = rsf_overlay_host_draw_target(context11.Get(), target.Get(), static_cast<uint32_t>(desc.Width), desc.Height, stats, intent);
    interop->ReleaseWrappedResources(acquired, 1); context11->Flush();
    return drawn;
}
void rsf_overlay_d3d12_stop()
{
    std::lock_guard<std::mutex> lock(drawing);
    rsf_overlay_host_stop(); interop.Reset(); context11.Reset(); device11.Reset(); owner.Reset(); ready = failed = false;
}
