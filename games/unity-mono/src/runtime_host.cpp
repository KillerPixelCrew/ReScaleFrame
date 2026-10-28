// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/unity_runtime_host.h>
#include <rescaleframe/unity_runtime_config.h>
#include <rescaleframe/callback_render_host.h>
#include <rescaleframe/native_fg_d3d12.h>
#include <rescaleframe/fg_choice.h>
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/sr_session.h>
#include <windows.h>
#include <d3d12.h>
#include <d3d12sdklayers.h>
#include <wrl/client.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
#include <mutex>
#include <string>

extern "C" int rsf_unity_set_engine_spatial(uint32_t, uint32_t) noexcept;
namespace {
std::mutex log_guard;
std::wstring log_path;
bool fg_started{};
void log(void*, const char* message)
{
    std::lock_guard lock(log_guard);
    if (FILE* file = _wfopen(log_path.c_str(), L"ab")) {
        std::fprintf(file, "%s\n", message); std::fclose(file);
    }
}
int accept_window(void*, void* pointer)
{
    DWORD process{}; wchar_t name[64]{};
    const auto window = static_cast<HWND>(pointer);
    return GetWindowThreadProcessId(window, &process) && process == GetCurrentProcessId() &&
        GetClassNameW(window, name, 64) && !std::wcscmp(name, L"UnityWndClass");
}
std::string backend_directory(const wchar_t* ini, uint32_t backend)
{
    const auto* info = rsf_fg_backend_info_for(backend);
    if (!info) return {};
    const std::wstring key(info->ini_key, info->ini_key + std::strlen(info->ini_key));
    return rsf::unity_config::utf8(rsf::unity_config::path_setting(ini, key.c_str()));
}
}
extern "C" uint32_t __stdcall rsf_unity_fg_start(const wchar_t* ini) try
{
    if (!ini || fg_started) return fg_started ? 0u : 1u;
    const auto backend = rsf::unity_config::fg_choice_start(ini);
    const auto directory = backend_directory(ini, backend && backend != RSF_FG_BACKEND_AUTO ? backend : RSF_FG_BACKEND_FSR3);
    if (directory.empty()) return 3;
    log_path = rsf::unity_config::path_setting(ini, L"Log");
    if (GetPrivateProfileIntW(L"UnitySR", L"GraphicsDebug", 0, ini)) {
        Microsoft::WRL::ComPtr<ID3D12Debug> debug;
        if (SUCCEEDED(D3D12GetDebugInterface(IID_PPV_ARGS(&debug)))) debug->EnableDebugLayer();
    }
    rsf_fg12_setup setup{}; setup.struct_size = sizeof(setup);
    setup.backend = backend; setup.max_generated_frames = UINT32_MAX;
    setup.options = {sizeof(setup.options), RSF_FG_FIXED,
        GetPrivateProfileIntW(L"UnitySR", L"GeneratedFrames", 1, ini), RSF_REFLEX_OFF, 0, 0};
    if (!setup.options.generated_frames || setup.options.generated_frames > 15) return 4;
    const auto limit = GetPrivateProfileIntW(L"UnitySR", L"FrameLimitFPS", 0, ini);
    setup.options.frame_limit_us = limit ? 1000000u / limit : 0;
    setup.depth_inverted = 1; setup.units_to_meters = 1; setup.accept_window = accept_window;
    setup.runtime_switching = 1; setup.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS;
    setup.engine_type = RSF_DLSS_ENGINE_UNITY; setup.engine_version_utf8 = RSF_UNITY_ENGINE_VERSION;
    setup.project_id_utf8 = RSF_UNITY_PROJECT_ID; setup.log = log;
    const auto fsr3 = backend_directory(ini, RSF_FG_BACKEND_FSR3);
    const auto fsr4 = backend_directory(ini, RSF_FG_BACKEND_FSR4);
    const auto xess = backend_directory(ini, RSF_FG_BACKEND_XESS);
    const auto streamline = backend_directory(ini, RSF_FG_BACKEND_DLSS);
    setup.runtime_directory_utf8 = directory.c_str();
    setup.fsr3_directory_utf8 = fsr3.c_str(); setup.fsr4_directory_utf8 = fsr4.c_str();
    setup.xess_directory_utf8 = xess.c_str(); setup.streamline_directory_utf8 = streamline.c_str();
    fg_started = rsf_fg12_install(&setup) == RSF_BACKEND_OK;
    const auto capture = rsf::unity_config::utf8(rsf::unity_config::path_setting(ini, L"FGCaptureDirectory"));
    if (fg_started && !capture.empty()) (void)rsf_fg12_capture_outputs(capture.c_str());
    log(nullptr, fg_started ? "Unity generation installed before graphics creation." : "Unity generation installation refused.");
    return fg_started ? 0u : 5u;
} catch (...) { return 5; }
extern "C" uint32_t __stdcall rsf_unity_sr_start(const wchar_t* ini) try
{
    if (!ini || !*ini) return 1;
    using rsf::unity_config::path_setting;
    using rsf::unity_config::utf8;
    if (!rsf_fg_choice_get().choices) rsf::unity_config::fg_choice_start(ini);
    const auto preferences = rsf::unity_config::preferences_path(ini);
    const auto plugin = path_setting(ini, L"Plugin"), output = path_setting(ini, L"Log");
    if (plugin.empty() || output.empty()) return 3;
    const auto streamline = utf8(path_setting(ini, L"Streamline")), fsr2 = utf8(path_setting(ini, L"FSR2"));
    const auto fsr3 = utf8(path_setting(ini, L"FSR3")), fsr4 = utf8(path_setting(ini, L"FSR4"));
    const auto xess = utf8(path_setting(ini, L"XeSS"));
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 5;
    const auto name = utf8(std::wstring(executable).substr(std::wstring(executable).find_last_of(L"/\\") + 1));
    std::string hash;
    if (!rsf::sha256_file(executable, hash)) return 6;
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0, name.c_str(), hash.c_str()};
    rsf_callback_host_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_CALLBACK_HOST_ABI_VERSION;
    setup.plugin_path = plugin.c_str(); setup.log_path = output.c_str(); setup.preferences_path = preferences.c_str();
    setup.probe = &probe;
    setup.backend = GetPrivateProfileIntW(L"UnitySR", L"Backend", RSF_SR_AUTO, ini);
    setup.sr_backend_choices = (1u << RSF_SR_DLSS) | (1u << RSF_SR_FSR2) | (1u << RSF_SR_FSR3) |
        (1u << RSF_SR_FSR4) | (1u << RSF_SR_XESS) | (1u << RSF_SR_SPATIAL);
    setup.quality = GetPrivateProfileIntW(L"UnitySR", L"Quality", 1, ini);
    setup.dlss_preset = GetPrivateProfileIntW(L"Rendering", L"DlssPreset",
        GetPrivateProfileIntW(L"UnitySR", L"DlssPreset", 0, ini), preferences.c_str());
    if (!rsf_dlss_preset_valid(setup.dlss_preset)) setup.dlss_preset = 0;
    setup.engine_type = RSF_DLSS_ENGINE_UNITY; setup.engine_version_utf8 = RSF_UNITY_ENGINE_VERSION;
    setup.project_id_utf8 = RSF_UNITY_PROJECT_ID;
    setup.streamline_directory_utf8 = streamline.c_str(); setup.fsr2_directory_utf8 = fsr2.c_str();
    setup.fsr3_directory_utf8 = fsr3.c_str(); setup.fsr4_directory_utf8 = fsr4.c_str(); setup.xess_directory_utf8 = xess.c_str();
    setup.set_engine_spatial = rsf_unity_set_engine_spatial;
    return rsf_callback_host_start(&setup);
} catch (...) { return 11; }
extern "C" uint32_t __stdcall rsf_unity_sr_stop(void*) { return rsf_callback_host_stop(); }
extern "C" uint32_t __stdcall rsf_unity_start(const wchar_t* ini) try
{
    if (!ini || !GetPrivateProfileIntW(L"UnitySR", L"AutoStart", 1, ini)) return 0;
    (void)rsf_unity_fg_start(ini);
    const auto deadline = GetTickCount64() + 90000;
    while (GetTickCount64() < deadline && !GetModuleHandleW(L"mono-2.0-bdwgc.dll")) Sleep(100);
    if (!GetModuleHandleW(L"mono-2.0-bdwgc.dll")) return 1;
    Sleep(2000);
    uint32_t result = 10;
    while (GetTickCount64() < deadline) {
        result = rsf_unity_sr_start(ini);
        if (result != 10) break;
        Sleep(500);
    }
    return result;
} catch (...) { return 1; }
