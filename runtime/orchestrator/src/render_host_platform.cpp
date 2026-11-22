// SPDX-License-Identifier: MIT
// DLSS registration, presentation bridge, overlay and observer operations
// for the Rust generic D3D12 render host in render_host.rs. Each leaf performs one platform call.
// Admission, host state, settings translation and commit, backend resolution, slot and fence
// bookkeeping, scene matching, status and overlay routing are decided in Rust. Returned COM
// interfaces carry one reference for the caller; arguments are borrowed for the call.
#include <rescaleframe/render_host.h>
#include <rescaleframe/render_frontend.h>
#include <rescaleframe/dlss_native12.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/overlay_fg_stats.h>
#include <rescaleframe/d3d12_observer.h>
#include <rescaleframe/sharpening_settings.h>
#include <dxgi1_4.h>
#include <windows.h>
#include <cstddef>

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
uint32_t fg_default_backend(uint32_t vendor, uint32_t software) { return rsf_fg_default_backend(vendor, software); }
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
    decltype(&native_object) native_object;
    decltype(&depth_domains) depth_domains;
    decltype(&dlss_preset_valid) dlss_preset_valid;
    decltype(&fg_default_backend) fg_default_backend;
    decltype(&registration_create) registration_create;
    decltype(&registration_borrow) registration_borrow;
    decltype(&registration_present) registration_present;
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
    native_object, depth_domains, dlss_preset_valid,
    fg_default_backend,
    registration_create,
    registration_borrow, registration_present,
    present_host, present_has_owner, present_request,
    present_backend_choices, present_backend, overlay_visible, overlay_toggle, overlay_present, overlay_fill_fg,
    observer_create, observer_device, observer_factory, diagnostics_enable,
};
}

extern "C" const Platform* rsf_render_host_platform()
{
    return &platform;
}
