// SPDX-License-Identifier: GPL-3.0-only
// Contract for an adapter without super resolution (sr_backend_choices == 0): SR can only be Off, requests to
// enable it or to change its backend/quality are refused without touching the INI, saved SR keys are replaced
// by the defaults, and status says so. Separate executable because the service configures once per process.
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/native_fg.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <string>

namespace {
int failures = 0;
void check(bool condition, const char* what)
{
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}
int changed_calls = 0;
void on_changed(void*, const rsf_render_settings*) { ++changed_calls; }
uint32_t read(const std::wstring& ini, const wchar_t* key)
{
    return GetPrivateProfileIntW(L"Anomaly", key, 0xdead, ini.c_str());
}
}

int main()
{
    wchar_t directory[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    const std::wstring ini = std::wstring(directory) + L"rsf_overlay_no_sr_" + std::to_wstring(GetCurrentProcessId()) + L".ini";
    DeleteFileW(ini.c_str());
    // A previous build saved SR on; this adapter cannot honour it.
    check(WritePrivateProfileStringW(L"Anomaly", L"SR", L"1", ini.c_str()) &&
        WritePrivateProfileStringW(L"Anomaly", L"SRBackend", L"4", ini.c_str()) &&
        WritePrivateProfileStringW(L"Anomaly", L"SRQuality", L"3", ini.c_str()) &&
        WritePrivateProfileStringW(L"Anomaly", L"StatusOverlay", L"1", ini.c_str()), "seed INI");

    rsf_overlay_api api{}; api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    check(rsf_get_overlay_api(&api) == RSF_OK, "table");

    rsf_overlay_service_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_OVERLAY_API_VERSION;
    setup.identity_utf8 = "no-sr fixture";
    setup.configuration_path = ini.c_str(); setup.configuration_section = L"Anomaly";
    setup.defaults = {sizeof(rsf_render_settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    setup.defaults.sr_backend = 1; setup.defaults.sr_quality = 0; setup.defaults.generated_frames = 1;
    setup.defaults.fg_backend = RSF_FG_BACKEND_AUTO; setup.defaults.latency = RSF_REFLEX_OFF;
    setup.sr_backend_choices = 0; setup.sr_quality_choices = 0;
    setup.fg_backend_choices = 0;
    setup.latency_choices = 1u << RSF_REFLEX_OFF;
    setup.max_generated_frames = 1;
    setup.settings_changed = on_changed;

    auto bad = setup; bad.defaults.sr_enabled = 1;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "defaults may not enable SR when it is unavailable");
    check(api.configure(&setup) == RSF_OK, "configure with no SR choices");

    rsf_render_settings settings{sizeof(settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    check(api.settings_get(&settings) == RSF_OK, "settings");
    check(settings.sr_enabled == 0 && settings.sr_backend == 1 && settings.sr_quality == 0, "saved SR keys replaced by the defaults");
    check(settings.status_overlay == 1, "unrelated saved settings survive");
    check(read(ini, L"SR") == 1, "loading does not rewrite the file");

    auto request = settings; request.sr_enabled = 1;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "enabling SR refused");
    request = settings; request.sr_backend = 3;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "SR backend change refused");
    request = settings; request.sr_quality = 2;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "SR quality change refused");
    check(changed_calls == 0 && read(ini, L"SR") == 1 && read(ini, L"SRBackend") == 4, "refusals changed nothing");
    request = settings; request.status_overlay = 0;
    check(api.settings_apply(&request) == RSF_OK && changed_calls == 1, "settings that do not involve SR still apply");
    check(read(ini, L"SR") == 0 && read(ini, L"SRBackend") == 1 && read(ini, L"StatusOverlay") == 0, "and persist SR as Off");

    rsf_render_status status{}; status.struct_size = sizeof(status); status.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    check(api.status(&status) == RSF_OK, "status");
    check(status.sr_enabled == 0 && status.sr_requested_backend == 0 && status.sr_effective_backend == 0 && status.sr_backend_choices == 0,
        "status reports SR off, no backend, no choices");
    // The reason is only meaningful once a Present has been sampled; before that the service waits for one.
    check(std::strcmp(status.reason, "Waiting for the first Present") == 0, "status reason before the first Present");

    DeleteFileW(ini.c_str());
    if (failures) return 1;
    std::puts("PASS: overlay DLL API contract for an adapter without super resolution");
    return 0;
}
