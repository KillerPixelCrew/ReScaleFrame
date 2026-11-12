// SPDX-License-Identifier: GPL-3.0-only
// DLL-import contract for rsf_get_overlay_api: table refusal, setup validation, settings
// persistence through the adapter's INI section and FG forwarding. No graphics device.
#include <rescaleframe/overlay_api.h>
#include <rescaleframe/native_fg.h>
#include <windows.h>
#include <cstdio>
#include <cstring>
#include <string>

#ifdef RSF_RUNTIME_BUILD
#error The contract client must import Runtime, never embed its owner.
#endif

namespace {
int failures = 0;
void check(bool condition, const char* what)
{
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}
int changed_calls = 0;
rsf_render_settings last_changed{};
void on_changed(void*, const rsf_render_settings* applied) { ++changed_calls; last_changed = *applied; }
std::wstring ini_path()
{
    wchar_t directory[MAX_PATH]{};
    GetTempPathW(MAX_PATH, directory);
    return std::wstring(directory) + L"rsf_overlay_dll_api_" + std::to_wstring(GetCurrentProcessId()) + L".ini";
}
uint32_t read(const std::wstring& ini, const wchar_t* key)
{
    return GetPrivateProfileIntW(L"Overlay", key, 0xdead, ini.c_str());
}
rsf_render_settings defaults()
{
    rsf_render_settings value{sizeof(value), RSF_RENDER_FRONTEND_ABI_VERSION};
    value.sr_enabled = 0; value.sr_backend = 1; value.dlss_preset = 0; value.fg_enabled = 0; value.fg_backend = RSF_FG_BACKEND_AUTO;
    value.generated_frames = 1; value.latency = RSF_REFLEX_OFF; value.frame_limit_us = 0; value.status_overlay = 0; value.sr_quality = 2;
    return value;
}
}

int main()
{
    // Query refusal: size first, then version; refusals write nothing.
    check(rsf_get_overlay_api(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "null table refused");
    rsf_overlay_api api{};
    std::memset(&api, 0xa5, sizeof(api));
    api.struct_size = sizeof(api) - 1; api.abi_version = 99;
    check(rsf_get_overlay_api(&api) == RSF_ERROR_INVALID_ARGUMENT, "short table refused before version");
    check(api.struct_size == sizeof(api) - 1 && api.abi_version == 99, "short refusal wrote nothing");
    api.struct_size = sizeof(api);
    check(rsf_get_overlay_api(&api) == RSF_ERROR_ABI_MISMATCH, "version mismatch refused");
    check(api.abi_version == 99 && reinterpret_cast<uintptr_t>(api.configure) == static_cast<uintptr_t>(0xa5a5a5a5a5a5a5a5ull),
        "version refusal wrote nothing");
    api = {}; api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    rsf_get_overlay_api_fn query = reinterpret_cast<rsf_get_overlay_api_fn>(
        GetProcAddress(GetModuleHandleW(L"ReScaleFrame.Runtime.dll"), RSF_OVERLAY_API_ENTRY_POINT));
    check(query == &rsf_get_overlay_api, "entry point exported by name");
    check(rsf_get_overlay_api(&api) == RSF_OK && api.struct_size == sizeof(api) && api.abi_version == RSF_OVERLAY_API_VERSION,
        "table filled");
    check(api.configure && api.present && api.notify && api.visible && api.set_visible && api.toggle && api.capture &&
        api.status && api.settings_get && api.settings_apply, "every entry populated");
    if (failures) return 1;

    // Unconfigured: nothing loads, nothing claims input.
    rsf_render_settings settings{sizeof(settings), RSF_RENDER_FRONTEND_ABI_VERSION};
    rsf_render_status status{}; status.struct_size = sizeof(status); status.abi_version = RSF_RENDER_FRONTEND_ABI_VERSION;
    rsf_overlay_capture capture{sizeof(capture)};
    check(api.settings_get(&settings) == RSF_ERROR_NOT_READY, "settings before configure");
    check(api.status(&status) == RSF_ERROR_NOT_READY, "status before configure");
    check(api.present(nullptr, nullptr) == RSF_ERROR_INVALID_ARGUMENT, "present null chain");
    check(api.set_visible(1) == RSF_ERROR_NOT_READY && api.toggle() == RSF_ERROR_NOT_READY, "visibility before start");
    check(api.notify(nullptr, RSF_OVERLAY_NOTIFY_RESIZE) == RSF_ERROR_NOT_READY, "notify before configure");
    check(api.capture(&capture) == RSF_OK && !capture.started && !capture.visible && !capture.captured, "nothing captured before start");
    capture.struct_size = 4;
    check(api.capture(&capture) == RSF_ERROR_INVALID_ARGUMENT, "short capture refused");
    check(api.visible() == 0, "not visible before start");

    const auto ini = ini_path();
    DeleteFileW(ini.c_str());
    // A foreign key must survive; an unsupported saved backend falls back to the default.
    check(WritePrivateProfileStringW(L"Overlay", L"Foreign", L"7", ini.c_str()) &&
        WritePrivateProfileStringW(L"Overlay", L"SRBackend", L"9", ini.c_str()) &&
        WritePrivateProfileStringW(L"Overlay", L"SRQuality", L"3", ini.c_str()), "seed INI");

    WritePrivateProfileStringW(L"Overlay", L"Sharpness", L"nan", ini.c_str());
    WritePrivateProfileStringW(L"Overlay", L"MotionScaleLimit", L"-4", ini.c_str());

    rsf_overlay_service_setup setup{};
    setup.struct_size = sizeof(setup); setup.abi_version = RSF_OVERLAY_API_VERSION;
    setup.identity_utf8 = "contract fixture";
    setup.configuration_path = ini.c_str(); setup.configuration_section = L"Overlay";
    setup.defaults = defaults();
    setup.sr_backend_choices = (1u << 1) | (1u << 3);
    setup.sr_quality_choices = 0x1fu;
    setup.fg_backend_choices = (1u << RSF_FG_BACKEND_FSR3) | (1u << RSF_FG_BACKEND_AUTO);
    setup.latency_choices = (1u << RSF_REFLEX_OFF) | (1u << RSF_REFLEX_ON);
    setup.max_generated_frames = 1;
    setup.settings_changed = on_changed;

    check(api.configure(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "null setup");
    auto bad = setup; bad.struct_size = sizeof(bad) - 4;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "short setup");
    bad = setup; bad.abi_version = RSF_OVERLAY_API_VERSION + 1;
    check(api.configure(&bad) == RSF_ERROR_ABI_MISMATCH, "setup version");
    bad = setup; bad.configuration_path = L"";
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "empty path");
    bad = setup; bad.configuration_section = nullptr;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "missing section");
    bad = setup; bad.sr_backend_choices = 1u << 0;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "SR backend bit 0 is not a backend");
    bad = setup; bad.sr_quality_choices = 0;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "SR backends without any quality");
    bad = setup; bad.sr_backend_choices = 0; bad.defaults.sr_enabled = 1;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "no SR but defaults enable SR");
    bad = setup; bad.latency_choices = 1u << RSF_REFLEX_ON;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "latency without Off");
    bad = setup; bad.fg_backend_choices = 1u << 2;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "unknown FG provider");
    bad = setup; bad.defaults.sr_backend = 5;
    check(api.configure(&bad) == RSF_ERROR_INVALID_ARGUMENT, "defaults outside choices");
    bad = setup; bad.defaults.abi_version = 1;
    check(api.configure(&bad) == RSF_ERROR_ABI_MISMATCH, "defaults version");
    check(api.settings_get(&settings) == RSF_ERROR_NOT_READY, "refused setups did not configure");

    check(api.configure(&setup) == RSF_OK, "configure");
    check(api.configure(&setup) == RSF_ERROR_BUSY, "second configure");
    check(api.settings_get(&settings) == RSF_OK, "settings after configure");
    check(settings.sr_backend == 1 && settings.sr_quality == 3 && settings.sr_enabled == 0 && settings.fg_backend == RSF_FG_BACKEND_AUTO,
        "saved quality read; unsupported saved backend replaced by default");
    check(settings.sharpening_mode == 0 && settings.sharpening_sharpness == 0.4f &&
        settings.sharpening_motion_scale_limit == 0.01f, "configure loads and sanitizes sharpening");
    check(read(ini, L"SRBackend") == 9, "load does not rewrite the file");

    auto request = settings;
    request.generated_frames = 2;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "generated above adapter maximum");
    request = settings; request.fg_enabled = 1; request.fg_backend = RSF_FG_BACKEND_DLSS;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "FG provider outside choices");
    request = settings; request.latency = RSF_REFLEX_BOOST;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "latency outside choices");
    request = settings; request.sr_backend = 4;
    check(api.settings_apply(&request) == RSF_ERROR_INVALID_ARGUMENT, "SR backend outside choices");
    request = settings; request.abi_version = 1;
    check(api.settings_apply(&request) == RSF_ERROR_ABI_MISMATCH, "settings version");
    check(api.settings_apply(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "null settings");
    check(changed_calls == 0 && read(ini, L"FG") == 0xdead, "refusals changed nothing");

    request = settings;
    request.sr_enabled = 1; request.sr_backend = 3; request.sr_quality = 1;
    request.fg_enabled = 1; request.fg_backend = RSF_FG_BACKEND_FSR3; request.latency = RSF_REFLEX_ON; request.frame_limit_us = 8333;
    check(api.settings_apply(&request) == RSF_OK, "valid apply");
    check(changed_calls == 1 && last_changed.sr_backend == 3 && last_changed.fg_backend == RSF_FG_BACKEND_FSR3, "adapter notified once");
    check(read(ini, L"SR") == 1 && read(ini, L"SRBackend") == 3 && read(ini, L"SRQuality") == 1 && read(ini, L"FG") == 1 &&
        read(ini, L"FGBackend") == RSF_FG_BACKEND_FSR3 && read(ini, L"Latency") == RSF_REFLEX_ON &&
        read(ini, L"FrameLimitUs") == 8333 && read(ini, L"Foreign") == 7, "persisted in adapter section; foreign key kept");
    rsf_render_settings reread{sizeof(reread), RSF_RENDER_FRONTEND_ABI_VERSION};
    check(api.settings_get(&reread) == RSF_OK && reread.sr_backend == 3 && reread.fg_enabled == 1, "get reflects apply");

    auto sharpen_request = reread;
    sharpen_request.sharpening_mode = 2; sharpen_request.sharpening_sharpness = 0.75f;
    sharpen_request.sharpening_mas = 1; sharpen_request.sharpening_motion_sharpness = -0.2f;
    sharpen_request.sharpening_motion_threshold = 3; sharpen_request.sharpening_motion_scale_limit = 12;
    sharpen_request.sharpening_contrast_enabled = 1; sharpen_request.sharpening_contrast = 1.5f;
    sharpen_request.sharpening_depth_scale = 72; sharpen_request.sharpening_depth_bias = 0.002f;
    sharpen_request.sharpening_clamp_output = 1; sharpen_request.sharpening_debug = 1;
    check(api.settings_apply(&sharpen_request) == RSF_OK, "sharpening applies and saves");
    check(api.settings_get(&reread) == RSF_OK && reread.sharpening_sharpness == 0.75f &&
        reread.sharpening_mode == 2 && read(ini,L"SharpeningMode") == 2 && read(ini,L"MAS") == 1,
        "sharpening service snapshot and INI reflect request");
    auto invalid_sharpening = sharpen_request; invalid_sharpening.sharpening_sharpness = 8;
    check(api.settings_apply(&invalid_sharpening) == RSF_ERROR_INVALID_ARGUMENT, "invalid sharpening refused");

    rsf_native_fg_status fg{}; fg.struct_size = sizeof(fg);
    check(rsf_native_fg_status_get(&fg) && fg.requested.mode == RSF_FG_FIXED && fg.requested.generated_frames == 1 &&
        fg.requested.reflex_mode == RSF_REFLEX_ON && fg.requested.frame_limit_us == 8333, "FG options forwarded to presentation owner");

    check(api.status(&status) == RSF_OK, "status");
    check(status.sr_enabled == 1 && status.sr_requested_backend == 3 && status.sr_effective_backend == 0 &&
        status.fg_requested == 1 && status.fg_requested_backend == RSF_FG_BACKEND_FSR3 && status.fg_effective_backend == 0 &&
        status.fg_active == 0 && status.latency_requested == RSF_REFLEX_ON && status.latency_effective == RSF_REFLEX_OFF &&
        status.sr_backend_choices == setup.sr_backend_choices, "requested and effective are separate");
    check(std::strcmp(status.reason, "Waiting for the first Present") == 0, "status reason before Present");
    status.abi_version = 1;
    check(api.status(&status) == RSF_ERROR_ABI_MISMATCH, "status version");

    check(api.notify(nullptr, 99) == RSF_ERROR_INVALID_ARGUMENT, "unknown notify reason");
    check(api.notify(nullptr, RSF_OVERLAY_NOTIFY_RESIZE) == RSF_OK && api.notify(nullptr, RSF_OVERLAY_NOTIFY_DEVICE_RESET) == RSF_OK,
        "resize and reset notifications");
    check(api.set_visible(1) == RSF_ERROR_NOT_READY, "configured but not started");

    DeleteFileW(ini.c_str());
    if (failures) return 1;
    std::puts("PASS: overlay DLL API contract through the real Runtime import");
    return 0;
}
