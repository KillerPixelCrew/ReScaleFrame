// SPDX-License-Identifier: MIT
// DLL export entry points and platform calls for the Rust overlay service.
#include <rescaleframe/adapter_settings.h>
#include <rescaleframe/d3d11_observer.h>
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/dlss_preset.h>
#include <rescaleframe/overlay_d3d12.h>
#include <rescaleframe/native_fg_d3d12.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include "overlay_bound.h"
#include "overlay_platform.h"
#include "render_settings_store.h"
#include <windows.h>
#include <dxgi.h>

extern "C" uint32_t rsf_core_overlay_refresh_mhz(void* window);

namespace {
void* window(void* swapchain, uint32_t* width, uint32_t* height) noexcept
{
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&desc))) return nullptr;
    *width = desc.BufferDesc.Width;
    *height = desc.BufferDesc.Height;
    return desc.OutputWindow;
}

uint32_t foreground() noexcept
{
    DWORD foreground{};
    GetWindowThreadProcessId(GetForegroundWindow(), &foreground);
    return foreground == GetCurrentProcessId();
}

uint64_t tick() noexcept { return GetTickCount64(); }

// These hosts can allocate while loading/drawing. Keep native exceptions at the native boundary.
int start(void* chain, rsf_game_log_fn log, void* user) noexcept try
{
    return rsf_overlay_host_start(chain, log, user) != 0;
} catch (...) { return -1; }

int present(void* chain, const rsf_overlay_stats* stats, rsf_overlay_intent* intent) noexcept try
{
    return rsf_overlay_host_present(chain, stats, intent) != 0;
} catch (...) { return -1; }

int stop() noexcept try
{
    rsf_overlay_host_stop();
    return 0;
} catch (...) { return -1; }

uint32_t valid_preset(uint32_t preset) noexcept { return rsf_dlss_preset_valid(preset); }

constexpr rsf_overlay_platform platform{
    .window = window, .refresh = rsf_core_overlay_refresh_mhz, .foreground = foreground, .tick = tick,
    .start = start, .present = present, .stop = stop,
    .initialize_sharpening = nullptr, .valid_settings = nullptr,
    .valid_preset = valid_preset, .sanitize_sharpening = nullptr,
    .set_sharpening = rsf_core_dlss_pipeline_apply_sharpening,
    .input_visible = rsf_overlay_input_visible, .input_set_visible = rsf_overlay_input_set_visible,
    .input_counters_get = rsf_overlay_input_counters_get, .input_routes_get = rsf_native_input_routes_get,
    .latency_status_get = rsf_native_latency_status_get, .fg_status_get = rsf_native_fg_status_get,
    .fg_options_set = rsf_native_fg_options_set, .present_has_owner = rsf_d3d11_present_has_owner,
    .present_provider = rsf_d3d11_present_provider, .present_session = rsf_d3d11_present_session,
    .present_backend = rsf_d3d11_present_backend, .present_backend_choices = rsf_d3d11_present_backend_choices,
    .present_switch_result = rsf_d3d11_present_switch_result, .present_request = rsf_d3d11_present_request,
};

const rsf_overlay_api& observed_panel()
{
    static const auto api = [] {
        rsf_overlay_api value{};
        value.struct_size = sizeof(value);
        value.abi_version = RSF_OVERLAY_API_VERSION;
        if (rsf_get_overlay_api(&value) != RSF_OK) value = {};
        return value;
    }();
    return api;
}

void observed_present(void*, const rsf_observer_present_event* event) noexcept try
{
    if (!event || event->completed || (event->flags & DXGI_PRESENT_TEST)) return;
    rsf_overlay_input_poll_toggle();
    (void)observed_panel().present(event->swapchain, nullptr);
} catch (...) {}
}

extern "C" {
int32_t rsf_core_adapter_setting_read_bool(const wchar_t*, const wchar_t*, const wchar_t*, uint32_t);
int32_t rsf_core_adapter_setting_write_bool(const wchar_t*, const wchar_t*, const wchar_t*, uint32_t);

RSF_RUNTIME_API int32_t rsf_adapter_setting_read_bool(
    const wchar_t* path, const wchar_t* section, const wchar_t* key, uint32_t fallback)
{
    return rsf_core_adapter_setting_read_bool(path, section, key, fallback);
}

RSF_RUNTIME_API int32_t rsf_adapter_setting_write_bool(
    const wchar_t* path, const wchar_t* section, const wchar_t* key, uint32_t value)
{
    return rsf_core_adapter_setting_write_bool(path, section, key, value);
}

rsf_result rsf_core_get_overlay_api(rsf_overlay_api*, const rsf_overlay_platform*);
rsf_result rsf_core_get_overlay_diagnostics(rsf_overlay_diagnostics*, const rsf_overlay_platform*);
rsf_result rsf_core_overlay_configure_deferred(const rsf_overlay_service_setup*);
rsf_result rsf_core_overlay_present_bound(void*, void*, int (*)(rsf_native_fg_status*),
    int (*)(void*, const rsf_overlay_stats*, rsf_overlay_intent*), void*);
rsf_result rsf_core_overlay_notify_bound(uint32_t, int (*)(void));

RSF_RUNTIME_API rsf_result rsf_get_overlay_api(rsf_overlay_api* api)
{
    return rsf_core_get_overlay_api(api, &platform);
}

RSF_RUNTIME_API rsf_result rsf_get_overlay_diagnostics(rsf_overlay_diagnostics* out)
{
    return rsf_core_get_overlay_diagnostics(out, &platform);
}

RSF_RUNTIME_API rsf_result rsf_start_d3d11_overlay(const rsf_overlay_service_setup* setup) try
{
    const auto& api = observed_panel();
    if (!api.configure || !api.present) return RSF_ERROR_NOT_READY;
    const auto configured = api.configure(setup);
    if (configured != RSF_OK) return configured;
    rsf_observer_options observer{};
    observer.struct_size = sizeof(observer);
    observer.abi_version = RSF_OBSERVER_ABI_VERSION;
    // The panel needs a Present event. Filter out all texture and buffer cataloguing.
    observer.format = UINT32_MAX;
    observer.minimum_width = UINT32_MAX;
    observer.capacity = 1;
    observer.on_present_event = observed_present;
    return rsf_observer_install(&observer) == RSF_OBSERVER_OK ? RSF_OK : RSF_ERROR_NOT_READY;
} catch (...) { return RSF_ERROR_NOT_READY; }
}

rsf_result rsf_overlay_configure_bound(const rsf_overlay_service_setup* setup)
{
    rsf_overlay_api api{};
    api.struct_size = sizeof(api);
    api.abi_version = RSF_OVERLAY_API_VERSION;
    const auto result = rsf_get_overlay_api(&api);
    return result == RSF_OK ? rsf_core_overlay_configure_deferred(setup) : result;
}

rsf_result rsf_overlay_present_bound(void* device, void* queue, void* chain, void* backbuffer, void* input_window,
    rsf_game_log_fn log, void* user)
{
    struct Frame {
        void* device;
        void* queue;
        void* backbuffer;
        void* window;
        rsf_game_log_fn log;
        void* user;
    } frame{device, queue, backbuffer, input_window, log, user};
    return rsf_core_overlay_present_bound(chain, input_window, rsf_fg12_status,
        [](void* context, const rsf_overlay_stats* stats, rsf_overlay_intent* intent) {
            const auto& f = *static_cast<Frame*>(context);
            return rsf_overlay_d3d12_frame_for_window(f.device, f.queue, f.backbuffer, f.window,
                stats, intent, f.log, f.user);
        }, &frame);
}

rsf_result rsf_overlay_notify_bound(uint32_t reason)
{
    return rsf_core_overlay_notify_bound(reason, []() -> int {
        rsf_overlay_d3d12_stop();
        return RSF_OK;
    });
}
