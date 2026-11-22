// SPDX-License-Identifier: MIT
// ABI adapters for the Rust OpenGL pipeline's vendor and explicit presentation calls.
#include <rescaleframe/gl_pipeline.h>
#include <rescaleframe/native_cpu.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/streamline_host.h>
#include <rescaleframe/d3d11_present_bridge.h>
#include "overlay_bound.h"
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
using Microsoft::WRL::ComPtr;

// Values gl_pipeline.rs names directly.
static_assert(RSF_SR_NONE == 0 && RSF_SR_DLSS == 1 && RSF_SR_FSR2 == 2 && RSF_SR_FSR3 == 3 && RSF_SR_FSR4 == 4 &&
              RSF_SR_XESS == 5 && RSF_SR_SPATIAL == 6 && RSF_SR_AUTO == 7);
static_assert(RSF_DLSS_ENGINE_CUSTOM == 0);
static_assert(D3D12_RESOURCE_STATE_COMMON == 0 && D3D12_RESOURCE_STATE_PRESENT == 0 &&
              D3D12_RESOURCE_STATE_RENDER_TARGET == 0x4 && D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE == 0x40 &&
              D3D12_RESOURCE_STATE_COPY_DEST == 0x400 && D3D12_RESOURCE_STATE_COPY_SOURCE == 0x800);
static_assert(DXGI_FORMAT_R8G8B8A8_UNORM == 28);

extern "C" int rsf_core_gl_window_valid(void*);
extern "C" int rsf_core_gl_window_child_of(void*, void*);
extern "C" int rsf_core_gl_window_resize(void*, uint32_t, uint32_t);
extern "C" void rsf_core_gl_window_show(void*);

extern "C" void rsf_core_gl_chain_associate(void*, void*);
extern "C" int32_t rsf_core_gl_chain_resize(void*, uint32_t, uint32_t, uint32_t, uint32_t);
extern "C" void* rsf_core_gl_chain_back_buffer(void*);
extern "C" int32_t rsf_core_gl_chain_present(void*, uint32_t);
extern "C" int rsf_core_gl_adapter_identity(void*, uint32_t*, uint32_t*);
extern "C" void rsf_core_gl_transition(void*, void*, uint32_t, uint32_t);
extern "C" void rsf_core_gl_copy_resource(void*, void*, void*);

namespace {
int32_t chain_create(void* queue, void* window, uint32_t buffers, uint32_t format, void** out)
{
    *out = nullptr;
    ComPtr<IDXGIFactory2> factory;
    ComPtr<IDXGISwapChain1> chain;
    ComPtr<IDXGISwapChain3> chain3;
    DXGI_SWAP_CHAIN_DESC1 desc{};
    desc.Width = desc.Height = 32;
    desc.Format = static_cast<DXGI_FORMAT>(format);
    desc.SampleDesc.Count = 1;
    desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
    desc.BufferCount = buffers;
    desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
    desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;
    auto result = CreateDXGIFactory1(IID_PPV_ARGS(&factory));
    if (SUCCEEDED(result))
        result = rsf_d3d11_present_create_for_hwnd(static_cast<IDXGIFactory*>(factory.Get()), queue, window,
                                                 &desc, reinterpret_cast<void**>(chain.GetAddressOf()));
    if (SUCCEEDED(result)) result = chain.As(&chain3);
    if (SUCCEEDED(result)) *out = chain3.Detach();
    return result;
}
rsf_backend_result gl_open(rsf_backend_log_fn log, void* user, rsf_gl_context** out)
{
    rsf_gl_setup sharing{sizeof(sharing), 1, nullptr, nullptr, log, user};
    return rsf_gl_open(&sharing, out);
}

int latency_completed(uint64_t frame)
{
    rsf_native_latency_frame timing{sizeof(timing), 1, frame};
    return rsf_native_latency_frame_get(&timing) && timing.completed;
}
uint32_t pcl_message()
{
    rsf_streamline_latency_status status{};
    status.struct_size = sizeof(status);
    return rsf_streamline_host_latency_status(rsf_d3d11_present_host(), &status) == RSF_BACKEND_OK ? status.pcl_message_id : 0;
}
void* present_host() { return rsf_d3d11_present_host(); }

// Header-inline contract predicates, kept single-sourced in the published headers.
int dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset); }
uint32_t fg_default_backend(uint32_t vendor, uint32_t software) { return rsf_fg_default_backend(vendor, software); }
uint32_t depth_domains_requested(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }

// Field order matches `Platform` in gl_pipeline.rs.
struct Platform {
    decltype(&gl_open) gl_open;
    decltype(&rsf_gl_graphics) gl_graphics;
    decltype(&rsf_gl_texture_create) gl_texture_create;
    decltype(&rsf_gl_texture_get) gl_texture_get;
    decltype(&rsf_gl_texture_destroy) gl_texture_destroy;
    decltype(&rsf_gl_begin) gl_begin;
    decltype(&rsf_gl_submit) gl_submit;
    decltype(&rsf_gl_drain) gl_drain;
    decltype(&rsf_gl_close) gl_close;
    decltype(&rsf_core_gl_window_valid) window_valid;
    decltype(&rsf_gl_window_create) window_create;
    decltype(&rsf_gl_window_destroy) window_destroy;
    decltype(&rsf_core_gl_window_child_of) window_child_of;
    decltype(&rsf_core_gl_window_resize) window_resize;
    decltype(&rsf_core_gl_window_show) window_show;
    decltype(&chain_create) chain_create;
    decltype(&rsf_core_gl_chain_associate) chain_associate;
    decltype(&rsf_core_gl_chain_resize) chain_resize;
    decltype(&rsf_core_gl_chain_back_buffer) chain_back_buffer;
    decltype(&rsf_core_gl_chain_present) chain_present;
    decltype(&rsf_core_gl_adapter_identity) adapter_identity;
    decltype(&rsf_core_gl_transition) transition;
    decltype(&rsf_core_gl_copy_resource) copy_resource;
    decltype(&rsf_sr12_create) sr_create;
    decltype(&rsf_sr12_plan) sr_plan;
    decltype(&rsf_sr12_evaluate) sr_evaluate;
    decltype(&rsf_sr12_set_sharpening) sr_set_sharpening;
    decltype(&rsf_sr12_destroy) sr_destroy;
    decltype(&rsf_fg12_install_explicit) fg_install;
    decltype(&rsf_fg12_options) fg_options;
    decltype(&rsf_fg12_cpu) fg_cpu;
    decltype(&rsf_fg12_hudless) fg_hudless;
    decltype(&rsf_fg12_submitted) fg_submitted;
    decltype(&rsf_fg12_window) fg_window;
    decltype(&rsf_fg12_status) fg_status;
    decltype(&present_host) present_host;
    decltype(&rsf_d3d11_present_is_owner) present_is_owner;
    decltype(&rsf_d3d11_present_backend) present_backend;
    decltype(&rsf_d3d11_present_backend_choices) present_backend_choices;
    decltype(&rsf_d3d11_present_request) present_request;
    decltype(&rsf_d3d11_present_switch_result) present_switch_result;
    decltype(&rsf_d3d11_present_abort) present_abort;
    decltype(&pcl_message) pcl_message;
    decltype(&rsf_native_cpu_read) cpu_read;
    decltype(&latency_completed) latency_completed;
    decltype(&rsf_native_latency_end_session) latency_end_session;
    decltype(&rsf_input_dump_cancel) dump_cancel;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&fg_default_backend) fg_default_backend;
    decltype(&depth_domains_requested) depth_domains_requested;
    decltype(&rsf_overlay_configure_bound) overlay_configure;
    decltype(&rsf_overlay_present_bound) overlay_present;
    decltype(&rsf_overlay_notify_bound) overlay_notify;
};

constexpr Platform platform{
    gl_open, rsf_gl_graphics, rsf_gl_texture_create, rsf_gl_texture_get, rsf_gl_texture_destroy, rsf_gl_begin,
    rsf_gl_submit, rsf_gl_drain, rsf_gl_close, rsf_core_gl_window_valid, rsf_gl_window_create, rsf_gl_window_destroy, rsf_core_gl_window_child_of,
    rsf_core_gl_window_resize, rsf_core_gl_window_show, chain_create, rsf_core_gl_chain_associate, rsf_core_gl_chain_resize, rsf_core_gl_chain_back_buffer, rsf_core_gl_chain_present,
    rsf_core_gl_adapter_identity, rsf_core_gl_transition, rsf_core_gl_copy_resource, rsf_sr12_create, rsf_sr12_plan, rsf_sr12_evaluate,
    rsf_sr12_set_sharpening, rsf_sr12_destroy, rsf_fg12_install_explicit, rsf_fg12_options, rsf_fg12_cpu, rsf_fg12_hudless,
    rsf_fg12_submitted, rsf_fg12_window, rsf_fg12_status, present_host, rsf_d3d11_present_is_owner,
    rsf_d3d11_present_backend, rsf_d3d11_present_backend_choices, rsf_d3d11_present_request,
    rsf_d3d11_present_switch_result, rsf_d3d11_present_abort,
    pcl_message, rsf_native_cpu_read, latency_completed, rsf_native_latency_end_session, rsf_input_dump_cancel,
    dlss_preset_valid, fg_default_backend, depth_domains_requested,
    rsf_overlay_configure_bound, rsf_overlay_present_bound, rsf_overlay_notify_bound,
};
}

extern "C" rsf_backend_result rsf_core_gl_pipeline_open(const rsf_gl_pipeline_setup*, rsf_gl_pipeline**, const Platform*);

extern "C" rsf_backend_result rsf_gl_pipeline_open(const rsf_gl_pipeline_setup* setup, rsf_gl_pipeline** out)
{
    return rsf_core_gl_pipeline_open(setup, out, &platform);
}
