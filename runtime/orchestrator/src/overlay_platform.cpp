// SPDX-License-Identifier: MIT
// DLL export entry points and platform calls for the Rust overlay service.
#include <rescaleframe/dlss_pipeline.h>
#include <rescaleframe/dlss_preset.h>
#include "../../../loader/proxy/src/overlay_host.h"
#include "overlay_platform.h"
#include "render_settings_store.h"
#include <windows.h>
#include <dxgi.h>

namespace {
void* window(void* swapchain, uint32_t* width, uint32_t* height) noexcept
{
    DXGI_SWAP_CHAIN_DESC desc{};
    if (FAILED(static_cast<IDXGISwapChain*>(swapchain)->GetDesc(&desc))) return nullptr;
    *width = desc.BufferDesc.Width;
    *height = desc.BufferDesc.Height;
    return desc.OutputWindow;
}

uint32_t refresh(void* window) noexcept
{
    MONITORINFOEXW info{}; info.cbSize = sizeof(info);
    DEVMODEW mode{}; mode.dmSize = sizeof(mode);
    if (GetMonitorInfoW(MonitorFromWindow(static_cast<HWND>(window), MONITOR_DEFAULTTOPRIMARY), &info) &&
        EnumDisplaySettingsW(info.szDevice, ENUM_CURRENT_SETTINGS, &mode) && mode.dmDisplayFrequency > 1)
        return mode.dmDisplayFrequency * 1000u;
    return 0;
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

void set_sharpening(const rsf_render_settings* settings) noexcept
{
    const auto options = rsf::render_settings::sharpening(*settings);
    (void)rsf_dlss_pipeline_set_sharpening(&options);
}

constexpr rsf_overlay_platform platform{
    .window = window, .refresh = refresh, .foreground = foreground, .tick = tick,
    .start = start, .present = present, .stop = stop,
    .initialize_sharpening = nullptr, .valid_settings = nullptr,
    .valid_preset = valid_preset, .sanitize_sharpening = nullptr,
    .set_sharpening = set_sharpening,
    .input_visible = rsf_overlay_input_visible, .input_set_visible = rsf_overlay_input_set_visible,
    .input_counters_get = rsf_overlay_input_counters_get, .input_routes_get = rsf_native_input_routes_get,
    .latency_status_get = rsf_native_latency_status_get, .fg_status_get = rsf_native_fg_status_get,
    .fg_options_set = rsf_native_fg_options_set, .present_has_owner = rsf_d3d11_present_has_owner,
    .present_provider = rsf_d3d11_present_provider, .present_session = rsf_d3d11_present_session,
    .present_backend = rsf_d3d11_present_backend, .present_backend_choices = rsf_d3d11_present_backend_choices,
    .present_switch_result = rsf_d3d11_present_switch_result, .present_request = rsf_d3d11_present_request,
};
}

extern "C" {
rsf_result rsf_core_get_overlay_api(rsf_overlay_api*, const rsf_overlay_platform*);
rsf_result rsf_core_get_overlay_diagnostics(rsf_overlay_diagnostics*, const rsf_overlay_platform*);

RSF_RUNTIME_API rsf_result rsf_get_overlay_api(rsf_overlay_api* api)
{
    return rsf_core_get_overlay_api(api, &platform);
}

RSF_RUNTIME_API rsf_result rsf_get_overlay_diagnostics(rsf_overlay_diagnostics* out)
{
    return rsf_core_get_overlay_diagnostics(out, &platform);
}
}
