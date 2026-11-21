// SPDX-License-Identifier: MIT
// Native leaves of the Anomaly overlay service. overlay.rs owns the binding, the configuration translation, the
// callbacks, the start state and the panel diagnostics and capture. This file keeps the COM calls on a presented chain,
// guarded hook installs, and forwards the C++ entry points the cold entry, the FG
// stage and the fixtures call.
#include "overlay_service.h"
#include "overlay_platform.h"
#include <d3d11.h>

static_assert(rsf::anomaly::overlay::capture_mouse == RSF_OVERLAY_CAPTURE_MOUSE);
static_assert(rsf::anomaly::overlay::capture_keyboard == RSF_OVERLAY_CAPTURE_KEYBOARD);
using rsf::anomaly::overlay::StartStatus;
static_assert(int(StartStatus::started) == RSF_ANOMALY_OVERLAY_STARTED);
static_assert(int(StartStatus::already_started) == RSF_ANOMALY_OVERLAY_ALREADY_STARTED);
static_assert(int(StartStatus::api_missing) == RSF_ANOMALY_OVERLAY_API_MISSING);
static_assert(int(StartStatus::api_refused) == RSF_ANOMALY_OVERLAY_API_REFUSED);
static_assert(int(StartStatus::configure_refused) == RSF_ANOMALY_OVERLAY_CONFIGURE_REFUSED);
static_assert(int(StartStatus::hooks_refused) == RSF_ANOMALY_OVERLAY_HOOKS_REFUSED);

namespace rsf::anomaly::overlay {
namespace {
uint32_t describe_chain(void* chain, rsf_anomaly_overlay_chain* out) noexcept {
    auto* swap = static_cast<IDXGISwapChain*>(chain);
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(swap->GetDesc(&desc))) return 0;
    ID3D11Device* device = nullptr;
    (void)swap->GetDevice(__uuidof(ID3D11Device), reinterpret_cast<void**>(&device));
    *out = {desc.OutputWindow, device, desc.BufferDesc.Width, desc.BufferDesc.Height};
    if (device) device->Release();
    return 1;
}

int32_t capture_backbuffer(void* chain, rsf_input_dump_named_texture_fn dump, const rsf_named_frame_dump* request,
    rsf_named_frame_dump_report* report) noexcept {
    auto* swap = static_cast<IDXGISwapChain*>(chain);
    ID3D11Device* device = nullptr;
    ID3D11DeviceContext* context = nullptr;
    ID3D11Texture2D* buffer = nullptr;
    if (SUCCEEDED(swap->GetDevice(__uuidof(ID3D11Device), reinterpret_cast<void**>(&device))) && device)
        device->GetImmediateContext(&context);
    if (context) (void)swap->GetBuffer(0, __uuidof(ID3D11Texture2D), reinterpret_cast<void**>(&buffer));
    const int32_t result = buffer && dump ? dump(context, buffer, request, report) : -1;
    if (buffer) buffer->Release();
    if (context) context->Release();
    if (device) device->Release();
    return result;
}

uint32_t install_input(const char** reason) noexcept { return install_input_hooks(input_gate(), reason) ? 1u : 0u; }
uint32_t install_present(const char** reason) noexcept {
    return install_present_hooks(present_sink(), reason) ? 1u : 0u;
}

constexpr rsf_anomaly_overlay_platform platform{&describe_chain, &capture_backbuffer, &install_input, &install_present};
}

StartStatus bind_api(rsf_get_overlay_api_fn query, rsf_overlay_api* out, const char** reason) noexcept {
    const char* unused = nullptr;
    return StartStatus(rsf_anomaly_overlay_bind(query, out, reason ? reason : &unused));
}

bool is_engine_window(HWND window) noexcept { return rsf_anomaly_overlay_engine_window(window) != 0; }

uint32_t query_capture() noexcept { return rsf_anomaly_overlay_capture_bits(nullptr); }

PresentSink present_sink() noexcept {
    return {[](void* user, IDXGISwapChain* chain) noexcept { rsf_anomaly_overlay_present(user, chain); },
        [](void* user, IDXGISwapChain* chain) noexcept { rsf_anomaly_overlay_resize(user, chain); }, nullptr};
}

// One gate for the process. Never destroyed: DirectInput calls can still arrive while the process exits.
InputGate& input_gate() noexcept {
    static auto* gate = new InputGate([](void* user) noexcept { return rsf_anomaly_overlay_capture_bits(user); }, nullptr);
    return *gate;
}

StartStatus start(const StartOptions& options, const char** reason) noexcept {
    const char* unused = nullptr;
    const rsf_anomaly_overlay_options forwarded{options.runtime, options.configuration, options.log, options.log_user,
        options.api_override, options.diagnostics_override, options.capture_override, options.install_hooks ? 1u : 0u};
    return StartStatus(rsf_anomaly_overlay_start(&forwarded, &platform, reason ? reason : &unused));
}
}
