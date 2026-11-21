// SPDX-License-Identifier: MIT
// D3D12, DXGI, SR12, DLSS registration, FG12, presentation bridge, overlay and observer operations
// for the Rust generic D3D12 render host in render_host.rs. Each leaf performs one platform call.
// Admission, host state, settings translation and commit, backend resolution, slot and fence
// bookkeeping, scene matching, status and overlay routing are decided in Rust. Returned COM
// interfaces carry one reference for the caller; arguments are borrowed for the call.
#include <rescaleframe/render_host.h>
#include <rescaleframe/render_frontend.h>
#include <rescaleframe/native_sr_d3d12.h>
#include <rescaleframe/dlss_native12.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/d3d12_observer.h>
#include <rescaleframe/sharpening_settings.h>
#include "../../backends/common/d3d12_helpers.h"
#include <dxgi1_4.h>
#include <windows.h>
#include <d3d12.h>
#include <wrl/client.h>
#include <cstddef>

using Microsoft::WRL::ComPtr;

// Layouts render_host.rs mirrors.
static_assert(sizeof(rsf_render_host_setup) == 96 && offsetof(rsf_render_host_setup, configuration_path) == 8 &&
              offsetof(rsf_render_host_setup, presentation_owner) == 32 &&
              offsetof(rsf_render_host_setup, engine_version_utf8) == 48 &&
              offsetof(rsf_render_host_setup, depth_inverted) == 64 && offsetof(rsf_render_host_setup, ui_mode) == 72 &&
              offsetof(rsf_render_host_setup, accept_window) == 80 && offsetof(rsf_render_host_setup, user) == 88);
static_assert(sizeof(rsf_d3d12_observer_setup) == 48 && offsetof(rsf_d3d12_observer_setup, user) == 8 &&
              offsetof(rsf_d3d12_observer_setup, accept_window) == 16 && offsetof(rsf_d3d12_observer_setup, submit) == 24 &&
              offsetof(rsf_d3d12_observer_setup, present) == 32 && offsetof(rsf_d3d12_observer_setup, swapchain) == 40);
static_assert(RSF_RENDER_HOST_ABI_VERSION == 1 && RSF_RENDER_HOST_ENGINE_PRESENTATION == 1 && RSF_SR_AUTO == 7 &&
              RSF_SR_DLSS == 1 && RSF_SR_XESS == 5 && RSF_FG_BACKEND_FSR3 == 3 && DXGI_PRESENT_TEST == 1 &&
              RSF_BACKEND_ERROR_NOT_READY == -8);

namespace {
struct Adapter {
    uint32_t vendor, device, software;
};

uint64_t tick() { return GetTickCount64(); }
uint32_t foreground()
{
    DWORD owner{};
    GetWindowThreadProcessId(GetForegroundWindow(), &owner);
    return owner == GetCurrentProcessId();
}
int32_t hide_cursor() { return ShowCursor(FALSE); }
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
int32_t queue_direct(void* queue)
{
    return static_cast<ID3D12CommandQueue*>(queue)->GetDesc().Type == D3D12_COMMAND_LIST_TYPE_DIRECT;
}
void* create_fence(void* device)
{
    ID3D12Fence* fence{};
    return SUCCEEDED(static_cast<ID3D12Device*>(device)->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&fence)))
        ? fence : nullptr;
}
uint64_t fence_completed(void* fence) { return static_cast<ID3D12Fence*>(fence)->GetCompletedValue(); }
// 1 when `value` completed within `timeout`, 0 when no wait event exists, -1 otherwise.
int32_t fence_wait(void* fence, uint64_t value, uint32_t timeout)
{
    HANDLE event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!event) return 0;
    const bool completed = SUCCEEDED(rsf::wait_fence(static_cast<ID3D12Fence*>(fence), value, event, timeout));
    CloseHandle(event);
    return completed ? 1 : -1;
}
int32_t queue_signal(void* queue, void* fence, uint64_t value)
{
    return SUCCEEDED(static_cast<ID3D12CommandQueue*>(queue)->Signal(static_cast<ID3D12Fence*>(fence), value));
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
// One owned reference to the physical object behind an engine's Streamline proxy.
void* native_object(void* object)
{
    using Native = int (*)(void*, void**);
    const auto module = GetModuleHandleW(L"sl.interposer.dll");
    const auto native = module ? reinterpret_cast<Native>(GetProcAddress(module, "slGetNativeInterface")) : nullptr;
    void* result{};
    if (native && !native(object, &result) && result) return result;
    static_cast<IUnknown*>(object)->AddRef();
    return object;
}
uint32_t depth_domains(const rsf_game_render_pass* pass) { return rsf_game_depth_domains_requested(&pass->depth_domains); }
uint32_t dlss_preset_valid(uint32_t preset) { return rsf_dlss_preset_valid(preset) ? 1u : 0u; }
void sharpening_defaults(rsf_sharpening_options* options) { *options = rsf_sharpening_defaults(); }
int32_t sharpening_valid(const rsf_sharpening_options* options) { return rsf::sharpening_settings::valid(*options); }
void sharpening_sanitize(rsf_sharpening_options* options) { *options = rsf::sharpening_settings::sanitize(*options); }
uint32_t fg_default_backend(uint32_t vendor, uint32_t software) { return rsf_fg_default_backend(vendor, software); }
int32_t sr12_create(const rsf_sr12_setup* setup, void** out)
{
    return rsf_sr12_create(setup, reinterpret_cast<rsf_sr12**>(out));
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
int32_t sr12_present(void* context, void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    return rsf_sr12_present(static_cast<rsf_sr12*>(context), swapchain, interval, flags, begin);
}
int32_t sr12_capture(void* context, const char* directory, uint64_t first_frame, uint32_t frames)
{
    return rsf_sr12_capture(static_cast<rsf_sr12*>(context), directory, first_frame, frames);
}
int32_t sr12_capture_flush(void* context, uint32_t slot) { return rsf_sr12_capture_flush(static_cast<rsf_sr12*>(context), slot); }
int32_t sr12_capture_scene(void* context, void* commands, const rsf_game_render_pass* pass, uint32_t slot)
{
    return rsf_sr12_capture_scene(static_cast<rsf_sr12*>(context), commands, pass, slot);
}
int32_t registration_create(void* device, const rsf_dlss_setup* setup, void** out)
{
    return rsf_dlss_native12_registration_create(device, setup, reinterpret_cast<rsf_dlss_native12_registration**>(out));
}
int32_t registration_borrow(void* device, const rsf_dlss_setup* setup, void** out)
{
    return rsf_dlss_native12_registration_borrow(device, GetModuleHandleW(L"sl.interposer.dll"), setup,
        reinterpret_cast<rsf_dlss_native12_registration**>(out));
}
int32_t registration_present(void* registration, void* swapchain, uint32_t interval, uint32_t flags, uint32_t begin)
{
    return rsf_dlss_native12_registration_present(static_cast<rsf_dlss_native12_registration*>(registration),
        swapchain, interval, flags, begin);
}
int32_t fg_install(const rsf_fg12_setup* setup) { return rsf_fg12_install(setup); }
void fg_options(const rsf_native_fg_options* options) { rsf_fg12_options(options); }
int32_t fg_latency(const rsf_game_latency_event* event) { return rsf_fg12_latency(event); }
void fg_cpu(const rsf_game_cpu_event* event) { rsf_fg12_cpu(event); }
void fg_submitted(uint64_t session, uint64_t frame) { rsf_fg12_submitted(session, frame); }
void fg_hudless(void* commands, const rsf_game_render_pass* pass) { rsf_fg12_hudless(commands, pass); }
void fg_window(const rsf_game_render_pass* pass) { rsf_fg12_window(pass); }
int32_t fg_status_try(rsf_native_fg_status* status) { return rsf_fg12_status_try(status); }
int32_t fg_capture_outputs(const char* directory) { return rsf_fg12_capture_outputs(directory); }
void* present_host() { return rsf_d3d11_present_host(); }
int32_t present_has_owner() { return rsf_d3d11_present_has_owner(); }
int32_t present_request(uint32_t backend) { return rsf_d3d11_present_request(backend); }
uint32_t present_backend_choices() { return rsf_d3d11_present_backend_choices(); }
uint32_t present_backend() { return rsf_d3d11_present_backend(); }
uint32_t overlay_visible() { return rsf_overlay_native12_visible(); }
void overlay_toggle() { rsf_overlay_native12_toggle(); }
int32_t overlay_present(void* device, void* queue, void* chain, const rsf_overlay_stats* stats, rsf_overlay_intent* intent,
    void (*log)(void*, const char*), void* user)
{
    return rsf_overlay_d3d12_present(device, queue, chain, stats, intent, log, user);
}
void overlay_fill_fg(rsf_overlay_stats* stats, const rsf_native_fg_status* fg) { rsf_overlay_fill_fg_stats(stats, fg); }
int32_t observer_create(const rsf_d3d12_observer_setup* setup, void** out)
{
    return rsf_d3d12_observer_create(setup, reinterpret_cast<rsf_d3d12_observer**>(out));
}
int32_t observer_device(void* observer, void* device)
{
    return rsf_d3d12_observer_device(static_cast<rsf_d3d12_observer*>(observer), device);
}
int32_t observer_factory(void* observer, void* factory)
{
    return rsf_d3d12_observer_factory(static_cast<rsf_d3d12_observer*>(observer), factory);
}
int32_t diagnostics_enable(void (*log)(void*, const char*), void* user) { return rsf_d3d12_diagnostics_enable(log, user); }

// Field order matches `Platform` in render_host.rs.
struct Platform {
    uint32_t struct_size;
    decltype(&tick) tick;
    decltype(&foreground) foreground;
    decltype(&hide_cursor) hide_cursor;
    decltype(&list_device) list_device;
    decltype(&queue_device) queue_device;
    decltype(&queue_direct) queue_direct;
    decltype(&create_fence) create_fence;
    decltype(&fence_completed) fence_completed;
    decltype(&fence_wait) fence_wait;
    decltype(&queue_signal) queue_signal;
    decltype(&adapter) adapter;
    decltype(&native_object) native_object;
    decltype(&depth_domains) depth_domains;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&sharpening_defaults) sharpening_defaults;
    decltype(&sharpening_valid) sharpening_valid;
    decltype(&sharpening_sanitize) sharpening_sanitize;
    decltype(&fg_default_backend) fg_default_backend;
    decltype(&sr12_create) sr12_create;
    decltype(&sr12_destroy) sr12_destroy;
    decltype(&sr12_evaluate) sr12_evaluate;
    decltype(&sr12_set_sharpening) sr12_set_sharpening;
    decltype(&sr12_present) sr12_present;
    decltype(&sr12_capture) sr12_capture;
    decltype(&sr12_capture_flush) sr12_capture_flush;
    decltype(&sr12_capture_scene) sr12_capture_scene;
    decltype(&registration_create) registration_create;
    decltype(&registration_borrow) registration_borrow;
    decltype(&registration_present) registration_present;
    decltype(&fg_install) fg_install;
    decltype(&fg_options) fg_options;
    decltype(&fg_latency) fg_latency;
    decltype(&fg_cpu) fg_cpu;
    decltype(&fg_submitted) fg_submitted;
    decltype(&fg_hudless) fg_hudless;
    decltype(&fg_window) fg_window;
    decltype(&fg_status_try) fg_status_try;
    decltype(&fg_capture_outputs) fg_capture_outputs;
    decltype(&present_host) present_host;
    decltype(&present_has_owner) present_has_owner;
    decltype(&present_request) present_request;
    decltype(&present_backend_choices) present_backend_choices;
    decltype(&present_backend) present_backend;
    decltype(&overlay_visible) overlay_visible;
    decltype(&overlay_toggle) overlay_toggle;
    decltype(&overlay_present) overlay_present;
    decltype(&overlay_fill_fg) overlay_fill_fg;
    decltype(&observer_create) observer_create;
    decltype(&observer_device) observer_device;
    decltype(&observer_factory) observer_factory;
    decltype(&diagnostics_enable) diagnostics_enable;
};

constexpr Platform platform{
    sizeof(Platform),
    tick, foreground, hide_cursor, list_device, queue_device, queue_direct, create_fence, fence_completed,
    fence_wait, queue_signal, adapter, native_object, depth_domains, dlss_preset_valid, sharpening_defaults,
    sharpening_valid, sharpening_sanitize, fg_default_backend, sr12_create, sr12_destroy, sr12_evaluate,
    sr12_set_sharpening, sr12_present, sr12_capture, sr12_capture_flush, sr12_capture_scene, registration_create,
    registration_borrow, registration_present, fg_install, fg_options, fg_latency, fg_cpu, fg_submitted, fg_hudless,
    fg_window, fg_status_try, fg_capture_outputs, present_host, present_has_owner, present_request,
    present_backend_choices, present_backend, overlay_visible, overlay_toggle, overlay_present, overlay_fill_fg,
    observer_create, observer_device, observer_factory, diagnostics_enable,
};
}

extern "C" {
int32_t rsf_core_render_host_create(const rsf_render_host_setup*, rsf_game_host_services*, const Platform*);
int32_t rsf_core_render_host_graphics(void*, void*);
int32_t rsf_core_render_host_device(void*, void*);
int32_t rsf_core_render_host_factory(void*, void*);
uint32_t rsf_core_render_host_graphics_ready(void*);
void* rsf_core_render_host_native_object(void*, void*);
int32_t rsf_core_render_status_get(rsf_render_status*);
int32_t rsf_core_render_settings_get(rsf_render_settings*);
int32_t rsf_core_render_settings_apply(const rsf_render_settings*);
}

// The public entries stay native so the Runtime DLL and every embedded consumer keep the same
// exported names and decorations.
extern "C" uint32_t rsf_render_overlay_visible() { return rsf_overlay_native12_visible(); }
extern "C" void rsf_render_overlay_toggle() { rsf_overlay_native12_toggle(); }
extern "C" int32_t rsf_render_host_create(const rsf_render_host_setup* setup, rsf_game_host_services* services)
{
    return rsf_core_render_host_create(setup, services, &platform);
}
extern "C" int32_t rsf_render_host_graphics(void* user, void* native_queue)
{
    return rsf_core_render_host_graphics(user, native_queue);
}
extern "C" int32_t rsf_render_host_device(void* user, void* device) { return rsf_core_render_host_device(user, device); }
extern "C" int32_t rsf_render_host_factory(void* user, void* factory) { return rsf_core_render_host_factory(user, factory); }
extern "C" uint32_t rsf_render_host_graphics_ready(void* user) { return rsf_core_render_host_graphics_ready(user); }
extern "C" void* rsf_render_host_native_object(void* user, void* object)
{
    return rsf_core_render_host_native_object(user, object);
}
extern "C" int32_t rsf_render_status_get(rsf_render_status* output) { return rsf_core_render_status_get(output); }
extern "C" int32_t rsf_render_settings_get(rsf_render_settings* output) { return rsf_core_render_settings_get(output); }
extern "C" int32_t rsf_render_settings_apply(const rsf_render_settings* request)
{
    return rsf_core_render_settings_apply(request);
}
