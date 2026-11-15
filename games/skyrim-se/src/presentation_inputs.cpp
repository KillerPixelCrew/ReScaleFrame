// SPDX-License-Identifier: GPL-3.0-only
#include "presentation_inputs.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>
#include <windows.h>
#include <d3d11.h>
#include <dxgi.h>
#include <wrl/client.h>
#include <atomic>
#include <cstring>

namespace rsf::skyrim {
namespace {
using DrawInterface = uintptr_t (*)(void*);
using Capture = decltype(&rsf_d3d11_host_capture_texture);
DrawInterface original{};
Capture capture{};
rsf_game_host_services host{};
std::atomic<bool> live{};
std::atomic<unsigned> calls{};
struct Identity {
    uint64_t session{}, frame{}, view{};
    uintptr_t chain{};
    uint32_t width{}, height{};
};
thread_local Identity identity;
void* target() { return reinterpret_cast<unsigned char*>(GetModuleHandleW(nullptr)) + 0x116ac80; }

void snapshot(bool after) {
    if (!live.load(std::memory_order_acquire) || !capture || !identity.session) return;
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    if (identity.frame != *reinterpret_cast<const uint32_t*>(base + 0x33363f4) ||
        identity.session != host.session_id) return;
    const auto window = *reinterpret_cast<const uintptr_t*>(base + 0x3330198);
    if (!window) return;
    auto* raw_chain = *reinterpret_cast<IDXGISwapChain**>(window + 0x18);
    if (!raw_chain || uintptr_t(raw_chain) != identity.chain) return;
    auto* context = *reinterpret_cast<ID3D11DeviceContext**>(base + 0x3331f30);
    if (!context) return;
    Microsoft::WRL::ComPtr<IDXGISwapChain> chain = raw_chain;
    Microsoft::WRL::ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(chain->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return;
    D3D11_TEXTURE2D_DESC desc{}; buffer->GetDesc(&desc);
    if (desc.Width != identity.width || desc.Height != identity.height || desc.SampleDesc.Count != 1) return;
    const rsf_d3d11_host_capture request{
        sizeof(request), RSF_D3D11_HOST_ABI_VERSION, identity.session, identity.frame, identity.view,
        after ? "native_after_interface" : "native_before_interface",
        after ? "Native backbuffer after interface drawing; later native output work may remain" :
            "Native backbuffer before interface drawing; candidate scene boundary, not yet FG eligible",
        buffer.Get(), 0, 1};
    // The runtime rejects unarmed or mismatched captures before readback. No GPU
    // resource from this native point survives the synchronous capture call.
    (void)capture(host.user, context, &request);
}
uintptr_t observe(void* ui) noexcept {
    calls.fetch_add(1, std::memory_order_acq_rel);
    const auto base = uintptr_t(GetModuleHandleW(nullptr));
    const bool primary = uintptr_t(ui) == *reinterpret_cast<const uintptr_t*>(base + 0x219e5c0);
    if (primary) { try { snapshot(false); } catch (...) {} }
    const auto result = original(ui);
    if (primary) { try { snapshot(true); } catch (...) {} }
    calls.fetch_sub(1, std::memory_order_acq_rel);
    return result;
}
}
bool presentation_inputs_guard() {
    constexpr unsigned char expected[]{0x4c,0x8b,0xdc,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x57,0x48,0x83,0xec,0x50};
    return std::memcmp(target(), expected, sizeof(expected)) == 0;
}
bool presentation_inputs_start(const rsf_game_host_services& services) {
    if (calls.load(std::memory_order_acquire)) return false;
    if (!original && (!presentation_inputs_guard() ||
        MH_CreateHook(target(), reinterpret_cast<void*>(&observe), reinterpret_cast<void**>(&original)) != MH_OK)) return false;
    capture = reinterpret_cast<Capture>(reinterpret_cast<void*>(GetProcAddress(
        GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), "rsf_d3d11_host_capture_texture")));
    if (!capture) return false;
    host = services;
    const auto result = MH_EnableHook(target());
    if (result != MH_OK && result != MH_ERROR_ENABLED) return false;
    live.store(true, std::memory_order_release); return true;
}
void presentation_inputs_quiesce() { live.store(false, std::memory_order_release); }
bool presentation_inputs_busy() { return calls.load(std::memory_order_acquire) != 0; }
bool presentation_inputs_stop() {
    presentation_inputs_quiesce();
    if (!original) return true;
    const auto result = MH_DisableHook(target());
    return (result == MH_OK || result == MH_ERROR_DISABLED) && !presentation_inputs_busy();
}
void identify_presentation_inputs(const rsf_game_render_pass& pass) {
    identity = {pass.session_id, pass.native_frame, pass.view_key, uintptr_t(pass.swapchain),
        pass.camera.output_width, pass.camera.output_height};
}
}
