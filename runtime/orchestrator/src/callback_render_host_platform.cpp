// SPDX-License-Identifier: MIT
// D3D12, DXGI, SR12, FG12, presentation bridge, overlay, input dump and plugin session operations
// for the Rust shared callback render host in callback_host.rs. Each leaf performs one call.
// Admission, refusal codes, lifecycle order, host state, renderer configuration, slot and scene
// handling, settings and overlay control are decided in Rust. Returned COM interfaces carry one
// reference for the caller; arguments are borrowed for the call.
#include <rescaleframe/callback_render_host.h>
#include <rescaleframe/input_dump.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/sharpening_settings.h>
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_4.h>
#include <wrl/client.h>
#include <cstddef>

using Microsoft::WRL::ComPtr;

// Layouts callback_host.rs mirrors.
static_assert(sizeof(rsf_callback_host_setup) == 128 && offsetof(rsf_callback_host_setup, probe) == 32 &&
              offsetof(rsf_callback_host_setup, backend) == 40 &&
              offsetof(rsf_callback_host_setup, engine_version_utf8) == 64 &&
              offsetof(rsf_callback_host_setup, xess_directory_utf8) == 112 &&
              offsetof(rsf_callback_host_setup, set_engine_spatial) == 120);
static_assert(sizeof(rsf_plugin_session_options) == 128 && offsetof(rsf_plugin_session_options, services) == 24);
static_assert(RSF_CALLBACK_HOST_ABI_VERSION == 1 && RSF_PLUGIN_SESSION_ABI_VERSION == 1 && RSF_SR_DLSS == 1 &&
              RSF_SR_XESS == 5 && RSF_SR_SPATIAL == 6 && RSF_SR_AUTO == 7 &&
              RSF_BACKEND_ERROR_NOT_SUPPORTED == -7 && RSF_GAME_RENDER_RECONFIGURE == 0x80000000u);

namespace {
struct Adapter {
    uint32_t vendor, device, software;
};
struct ResourceDesc {
    uint64_t width;
    uint32_t height, format, samples;
};

uint64_t tick() { return GetTickCount64(); }
void* list_device(void* list)
{
    ID3D12Device* device{};
    return SUCCEEDED(static_cast<ID3D12GraphicsCommandList*>(list)->GetDevice(IID_PPV_ARGS(&device))) ? device : nullptr;
}
void* queue_device(void* queue)
{
    ID3D12Device* device{};
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->GetDevice(IID_PPV_ARGS(&device))) ? device : nullptr;
}
// The adapter that owns `device`.
int32_t adapter(void* device, Adapter* out)
{
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> owner;
    DXGI_ADAPTER_DESC1 desc{};
    if (FAILED(CreateDXGIFactory1(IID_PPV_ARGS(&factory))) ||
        FAILED(factory->EnumAdapterByLuid(static_cast<ID3D12Device*>(device)->GetAdapterLuid(), IID_PPV_ARGS(&owner))) ||
        FAILED(owner->GetDesc1(&desc))) return 0;
    *out = {desc.VendorId, desc.DeviceId, (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE) != 0 ? 1u : 0u};
    return 1;
}
int32_t last_present_count(void* swapchain, uint32_t* out)
{
    UINT count = 0;
    if (!swapchain || FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetLastPresentCount(&count))) return 0;
    *out = count;
    return 1;
}
void resource_desc(void* resource, ResourceDesc* out)
{
    const auto desc = static_cast<ID3D12Resource*>(resource)->GetDesc();
    *out = {desc.Width, desc.Height, static_cast<uint32_t>(desc.Format), desc.SampleDesc.Count};
}
uint32_t depth_domains(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }
uint32_t dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
void sharpening_defaults(rsf_sharpening_options* options) { *options = rsf_sharpening_defaults(); }
int32_t sharpening_valid(const rsf_sharpening_options* options) { return rsf::sharpening_settings::valid(*options); }
void sharpening_sanitize(rsf_sharpening_options* options) { *options = rsf::sharpening_settings::sanitize(*options); }
int32_t sr12_create(const rsf_sr12_setup* setup, void** out)
{
    return rsf_sr12_create(setup, reinterpret_cast<rsf_sr12**>(out));
}
int32_t sr12_plan(void* context, uint32_t* width, uint32_t* height)
{
    return rsf_sr12_plan(static_cast<const rsf_sr12*>(context), width, height);
}
void sr12_destroy(void* context) { rsf_sr12_destroy(static_cast<rsf_sr12*>(context)); }
int32_t sr12_evaluate(void* context, void* commands, const rsf_game_render_pass* pass, uint32_t slot)
{
    return rsf_sr12_evaluate(static_cast<rsf_sr12*>(context), commands, pass, slot);
}
int32_t sr12_set_sharpening(void* context, const rsf_sharpening_options* options)
{
    return rsf_sr12_set_sharpening(static_cast<rsf_sr12*>(context), options);
}
void fg_cpu(const rsf_game_cpu_event* event) { rsf_fg12_cpu(event); }
void fg_submitted(uint64_t session, uint64_t frame) { rsf_fg12_submitted(session, frame); }
void fg_hudless(void* commands, const rsf_game_render_pass* pass) { rsf_fg12_hudless(commands, pass); }
void fg_window(const rsf_game_render_pass* pass) { rsf_fg12_window(pass); }
int32_t fg_status(rsf_native_fg_status* status) { return rsf_fg12_status(status); }
void fg_options(const rsf_native_fg_options* options) { rsf_fg12_options(options); }
int32_t fg_choice_save(uint32_t backend) { return rsf_fg_choice_save(backend); }
void* present_host() { return rsf_d3d11_present_host(); }
int32_t present_request(uint32_t backend) { return rsf_d3d11_present_request(backend); }
uint32_t present_backend() { return rsf_d3d11_present_backend(); }
int32_t present_switch_result() { return rsf_d3d11_present_switch_result(); }
int32_t overlay_frame(void* device, void* queue, void* chain, const rsf_overlay_stats* stats, rsf_overlay_intent* intent,
    void (*log)(void*, const char*), void* user)
{
    return rsf_overlay_d3d12_frame(device, queue, chain, stats, intent, log, user);
}
void overlay_stop() { rsf_overlay_d3d12_stop(); }
void overlay_fill_fg(rsf_overlay_stats* stats, const rsf_native_fg_status* fg) { rsf_overlay_fill_fg_stats(stats, fg); }
void input_dump_cancel(uint64_t session) { rsf_input_dump_cancel(session); }
int32_t session_prepare(const rsf_plugin_session_options* options, void** out)
{
    return rsf_plugin_session_prepare(options, reinterpret_cast<rsf_plugin_session**>(out));
}
int32_t session_start(void* session) { return rsf_plugin_session_start(static_cast<rsf_plugin_session*>(session)); }
int32_t session_quiesce(void* session) { return rsf_plugin_session_quiesce(static_cast<rsf_plugin_session*>(session)); }
int32_t session_stop(void* session) { return rsf_plugin_session_stop(static_cast<rsf_plugin_session*>(session)); }

// Field order matches `Platform` in callback_host.rs.
struct Platform {
    uint32_t struct_size;
    decltype(&tick) tick;
    decltype(&list_device) list_device;
    decltype(&queue_device) queue_device;
    decltype(&adapter) adapter;
    decltype(&last_present_count) last_present_count;
    decltype(&resource_desc) resource_desc;
    decltype(&depth_domains) depth_domains;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&sharpening_defaults) sharpening_defaults;
    decltype(&sharpening_valid) sharpening_valid;
    decltype(&sharpening_sanitize) sharpening_sanitize;
    decltype(&sr12_create) sr12_create;
    decltype(&sr12_plan) sr12_plan;
    decltype(&sr12_destroy) sr12_destroy;
    decltype(&sr12_evaluate) sr12_evaluate;
    decltype(&sr12_set_sharpening) sr12_set_sharpening;
    decltype(&fg_cpu) fg_cpu;
    decltype(&fg_submitted) fg_submitted;
    decltype(&fg_hudless) fg_hudless;
    decltype(&fg_window) fg_window;
    decltype(&fg_status) fg_status;
    decltype(&fg_options) fg_options;
    decltype(&fg_choice_save) fg_choice_save;
    decltype(&present_host) present_host;
    decltype(&present_request) present_request;
    decltype(&present_backend) present_backend;
    decltype(&present_switch_result) present_switch_result;
    decltype(&overlay_frame) overlay_frame;
    decltype(&overlay_stop) overlay_stop;
    decltype(&overlay_fill_fg) overlay_fill_fg;
    decltype(&input_dump_cancel) input_dump_cancel;
    decltype(&session_prepare) session_prepare;
    decltype(&session_start) session_start;
    decltype(&session_quiesce) session_quiesce;
    decltype(&session_stop) session_stop;
};

constexpr Platform platform{
    sizeof(Platform),
    tick, list_device, queue_device, adapter, last_present_count, resource_desc, depth_domains, dlss_preset_valid,
    sharpening_defaults, sharpening_valid, sharpening_sanitize, sr12_create, sr12_plan, sr12_destroy, sr12_evaluate,
    sr12_set_sharpening, fg_cpu, fg_submitted, fg_hudless, fg_window, fg_status, fg_options, fg_choice_save,
    present_host, present_request, present_backend, present_switch_result, overlay_frame, overlay_stop,
    overlay_fill_fg, input_dump_cancel, session_prepare, session_start, session_quiesce, session_stop,
};
}

extern "C" {
uint32_t rsf_core_callback_host_start(const rsf_callback_host_setup*, const Platform*);
uint32_t rsf_core_callback_host_stop(void);
}

// The public entries stay native so the Runtime DLL and every embedded consumer keep the same
// exported names and decorations.
extern "C" uint32_t rsf_callback_host_start(const rsf_callback_host_setup* setup)
{
    return rsf_core_callback_host_start(setup, &platform);
}
extern "C" uint32_t rsf_callback_host_stop(void) { return rsf_core_callback_host_stop(); }
