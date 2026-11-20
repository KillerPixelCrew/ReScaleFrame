// SPDX-License-Identifier: MIT
#include "fg_stage.h"
#include "motion_stage.h"
#include "sr_pass.h"
#include "sr_stage.h"
#include <d3d11.h>
#include <wrl/client.h>
#include <cstring>

namespace rsf::anomaly::fg {
namespace {
using Microsoft::WRL::ComPtr;

// The caller passes HW.pBaseZB from the exact executable profile the carrier selected.
void* engine_depth_view(uint32_t rva) noexcept {
    const auto* image = reinterpret_cast<const std::byte*>(GetModuleHandleW(nullptr));
    void* view = nullptr;
    if (image && rva) std::memcpy(&view, image + rva, sizeof(view));
    return view;
}

void* immediate_context(void* chain) noexcept {
    ComPtr<ID3D11Device> device;
    ID3D11DeviceContext* context{};
    if (SUCCEEDED(static_cast<IDXGISwapChain*>(chain)->GetDevice(IID_PPV_ARGS(&device))))
        device->GetImmediateContext(&context);
    return context;
}

void* backbuffer(void* chain) noexcept {
    ComPtr<ID3D11Texture2D> buffer;
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetBuffer(0, IID_PPV_ARGS(&buffer)))) return nullptr;
    return buffer.Detach();
}

void* depth_texture(void* view) noexcept {
    ComPtr<ID3D11Resource> resource;
    ComPtr<ID3D11Texture2D> depth;
    static_cast<ID3D11DepthStencilView*>(view)->GetResource(&resource);
    if (!resource || FAILED(resource.As(&depth))) return nullptr;
    return depth.Detach();
}

bool output_window(void* chain, uintptr_t* window) noexcept {
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(chain)->GetDesc(&desc))) return false;
    *window = reinterpret_cast<uintptr_t>(desc.OutputWindow);
    return true;
}

const rsf_anomaly_fg_platform& platform() noexcept {
    static const rsf_anomaly_fg_platform value{
        &input_platform(), sr::engine_version, sr::project_id, sr::view_space_to_meters,
        [](const rsf_anomaly_fg_overlay_link* link) { overlay_link.store(link, std::memory_order_release); },
        &engine_depth_view,
        [] { return sr::options().world_window(); },
        [] {
            const auto target = motion::frame_target();
            return rsf_anomaly_fg_motion_target{target.texture, target.width, target.height, target.frame, target.written, target.main};
        },
        &immediate_context, &backbuffer, &depth_texture, &output_window,
    };
    return value;
}
}

bool configure_ini(const wchar_t* ini, bool apply, HMODULE runtime, const BuildProfile& build,
    rsf_game_log_fn log) noexcept {
    return rsf_anomaly_fg_configure_ini(ini, apply, runtime, build.sr->hw_base_zb_rva, log, &platform());
}

void configure(const Options& options, rsf_game_log_fn log, void* user) noexcept {
    const rsf_anomaly_fg_config config{
        options.bridge, options.enabled, options.debug, options.development, options.capture,
        options.backend, options.generated, options.latency, options.frame_limit_us, options.scope_hold,
        options.capture_start, options.capture_count, options.capture_interval, options.capture_dir.c_str(),
        options.streamline_dir.c_str(), options.fidelityfx_dir.c_str(), options.xess_dir.c_str(),
    };
    rsf_anomaly_fg_configure(&config, log, user);
}

bool install(HMODULE runtime, const BuildProfile& build) noexcept {
    return rsf_anomaly_fg_install(runtime, build.sr->hw_base_zb_rva, &platform());
}
bool install_with(const rsf_d3d11_present_api& present, const rsf_d3d11_fg_api& api) noexcept {
    return rsf_anomaly_fg_install_with(&present, &api, &platform());
}
bool installed() noexcept { return rsf_anomaly_fg_installed(); }
void set_depth_view_source(void* (*source)()) noexcept { rsf_anomaly_fg_set_depth_source(source); }
void set_overlay_sink(const overlay::PresentSink& sink) noexcept {
    rsf_anomaly_fg_set_overlay_sink(sink.before_present, sink.user);
}
void camera(const float* view, const float* projection) noexcept { rsf_anomaly_fg_stage_camera(view, projection); }
void input_begin(uint32_t frame) noexcept { rsf_anomaly_fg_stage_input_begin(frame); }
void input_received(uint32_t kinds, uint32_t message) noexcept { rsf_anomaly_fg_stage_input_received(kinds, message); }
void render_begin(uint32_t frame, uint32_t width, uint32_t height) noexcept {
    rsf_anomaly_fg_stage_render_begin(frame, width, height);
}
void scope_frame() noexcept { rsf_anomaly_fg_stage_scope(); }
void ui_boundary() noexcept { rsf_anomaly_fg_stage_ui_boundary(); }
void end_before() noexcept { rsf_anomaly_fg_stage_end_before(); }
void end_after() noexcept { rsf_anomaly_fg_stage_end_after(); }
void device_reset() noexcept { rsf_anomaly_fg_stage_device_reset(); }
void report(void (*emit)(void*, const char*), void* user) noexcept { rsf_anomaly_fg_stage_report(emit, user); }
Status status() noexcept { return rsf_anomaly_fg_stage_status(); }
}
