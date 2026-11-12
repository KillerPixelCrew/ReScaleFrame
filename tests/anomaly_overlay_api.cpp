// SPDX-License-Identifier: GPL-3.0-only
// Contract for how the Anomaly adapter binds and configures the Runtime's shared overlay API. It uses
// fake API tables, so it proves refusal paths and the exact data the adapter hands the Runtime, not
// that the panel works. No hook is installed here (install_hooks=false or the start refuses first).
#include "overlay_service.h"
#include <rescaleframe/frame_generation.h>
#include <rescaleframe/sr_session.h>
#include <cstdio>
#include <cstring>
#include <string>

using namespace rsf::anomaly::overlay;

namespace {
int failures = 0;
void check(bool condition, const char* what) {
    if (!condition) { std::printf("FAIL: %s\n", what); ++failures; }
}

rsf_overlay_service_setup recorded{};
std::wstring recorded_path, recorded_section;
std::string recorded_identity;
int configure_calls = 0;
int32_t configure_result = RSF_OK;
uint32_t capture_result_bits = 0;
int32_t capture_result = RSF_OK;
std::string last_log;

int32_t fake_configure(const rsf_overlay_service_setup* setup) {
    ++configure_calls;
    recorded = *setup;
    recorded_path = setup->configuration_path;
    recorded_section = setup->configuration_section;
    recorded_identity = setup->identity_utf8;
    return configure_result;
}
int32_t fake_present(void*, uint32_t* drawn) { if (drawn) *drawn = 0; return RSF_OK; }
int32_t fake_notify(void*, uint32_t) { return RSF_OK; }
uint32_t fake_visible() { return 0; }
int32_t fake_set_visible(uint32_t) { return RSF_OK; }
int32_t fake_toggle() { return RSF_OK; }
int32_t fake_capture(rsf_overlay_capture* out) {
    if (capture_result != RSF_OK) return capture_result;
    out->captured = capture_result_bits;
    return RSF_OK;
}
int32_t fake_status(rsf_render_status*) { return RSF_OK; }
int32_t fake_settings_get(rsf_render_settings* settings) {
    settings->sr_enabled = 0; settings->fg_enabled = 0; settings->status_overlay = 0;
    return RSF_OK;
}
int32_t fake_settings_apply(const rsf_render_settings*) { return RSF_OK; }
void record_log(void*, const char* message) { last_log = message; }

rsf_overlay_api good_table() {
    rsf_overlay_api api{};
    api.struct_size = sizeof(api); api.abi_version = RSF_OVERLAY_API_VERSION;
    api.configure = fake_configure; api.present = fake_present; api.notify = fake_notify; api.visible = fake_visible;
    api.set_visible = fake_set_visible; api.toggle = fake_toggle; api.capture = fake_capture; api.status = fake_status;
    api.settings_get = fake_settings_get; api.settings_apply = fake_settings_apply;
    return api;
}

rsf_overlay_api query_table;
rsf_result query_result = RSF_OK;
rsf_result fake_query(rsf_overlay_api* out) {
    if (query_result == RSF_OK) *out = query_table;
    return query_result;
}

void binding_refuses_cleanly() {
    rsf_overlay_api out{};
    out.struct_size = 0x1234;
    const char* reason = nullptr;
    check(bind_api(nullptr, &out, &reason) == StartStatus::api_missing && reason && out.struct_size == 0x1234, "missing export refused");

    query_result = RSF_ERROR_ABI_MISMATCH; query_table = good_table();
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused && out.struct_size == 0x1234, "ABI mismatch refused, table untouched");
    check(reason && std::strstr(reason, "version"), "mismatch names the version");
    query_result = RSF_ERROR_INVALID_ARGUMENT;
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused, "a refused query is refused");

    query_result = RSF_OK;
    query_table = good_table(); query_table.abi_version = RSF_OVERLAY_API_VERSION + 1;
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused, "table of another version refused");
    query_table = good_table(); query_table.struct_size = sizeof(rsf_overlay_api) - 8;
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused, "short table refused");
    query_table = good_table(); query_table.capture = nullptr;
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused && reason && std::strstr(reason, "missing"), "table without capture refused");
    query_table = good_table(); query_table.present = nullptr;
    check(bind_api(&fake_query, &out, &reason) == StartStatus::api_refused, "table without present refused");
    check(out.struct_size == 0x1234, "no refusal wrote the output");

    query_table = good_table();
    check(bind_api(&fake_query, &out, &reason) == StartStatus::started && out.configure == fake_configure, "a complete table binds");
}

void start_refuses_without_the_runtime_api() {
    StartOptions options{};
    options.configuration = L"C:\\rsf-test\\ReScaleFrame.ini";
    options.log = record_log;
    const char* reason = nullptr;
    // A module that exists but has no overlay export (the real Runtime would carry it).
    options.runtime = GetModuleHandleW(L"kernel32.dll");
    configure_calls = 0;
    check(start(options, &reason) == StartStatus::api_missing && configure_calls == 0, "Runtime without the export refused before configure");
    check(last_log.find("does not export") != std::string::npos, "refusal is logged for the player");
    options.runtime = nullptr;
    check(start(options, &reason) == StartStatus::api_missing, "no Runtime handle refused");

    auto api = good_table();
    api.abi_version = 99;
    options.api_override = &api;
    check(start(options, &reason) == StartStatus::api_refused && configure_calls == 0, "version-mismatched table refused before configure");
    api = good_table(); api.toggle = nullptr;
    check(start(options, &reason) == StartStatus::api_refused && configure_calls == 0, "incomplete table refused before configure");

    api = good_table();
    options.configuration = L"";
    check(start(options, &reason) == StartStatus::configure_refused && configure_calls == 0, "empty configuration path refused");
    options.configuration = L"C:\\rsf-test\\ReScaleFrame.ini";
    options.install_hooks = false;
    configure_result = RSF_ERROR_INVALID_ARGUMENT;
    check(start(options, &reason) == StartStatus::configure_refused && configure_calls == 1, "a Runtime that rejects the setup is refused");
    check(last_log.find("rejected") != std::string::npos, "rejection is logged");
    configure_result = RSF_OK;
}

HWND window_with_class(const wchar_t* name) {
    WNDCLASSW cls{};
    cls.lpfnWndProc = DefWindowProcW; cls.hInstance = GetModuleHandleW(nullptr); cls.lpszClassName = name;
    RegisterClassW(&cls);
    return CreateWindowExW(0, name, L"t", WS_POPUP, 0, 0, 8, 8, nullptr, nullptr, GetModuleHandleW(nullptr), nullptr);
}

void start_hands_over_what_the_adapter_can_apply() {
    StartOptions options{};
    options.configuration = L"C:\\rsf-test\\ReScaleFrame.ini";
    options.log = record_log;
    options.install_hooks = false;
    auto api = good_table();
    options.api_override = &api;
    const char* reason = nullptr;
    configure_calls = 0;
    check(start(options, &reason) == StartStatus::started && configure_calls == 1, "a good table configures exactly once");
    check(recorded_identity == "Anomaly", "identity");
    check(recorded_path == L"C:\\rsf-test\\ReScaleFrame.ini" && recorded_section == L"Anomaly", "INI path and [Anomaly] section");
    check(recorded.abi_version == RSF_OVERLAY_API_VERSION && recorded.struct_size == sizeof(recorded), "setup is size/version stamped");
    check(recorded.fg_backend_choices == 0, "no FG backend is offered (FG is not wired)");
    check(recorded.latency_choices == (1u << RSF_REFLEX_OFF), "only latency Off is offered");
    check(recorded.max_generated_frames == 1, "no generated-frame budget beyond the minimum");
    check(recorded.dlss_preset_available == 0, "no DLSS preset claim");
    check(recorded.sr_backend_choices == 0 && recorded.sr_quality_choices == 0, "no SR choice is offered: SR is unavailable");
    check(recorded.defaults.sr_backend == RSF_SR_NONE, "no placeholder SR backend in the defaults");
    check(recorded.sr_report == nullptr, "no SR output is ever reported");
    check(recorded.defaults.sr_enabled == 0 && recorded.defaults.fg_enabled == 0, "SR and FG default off");
    check(recorded.defaults.struct_size == sizeof(rsf_render_settings) && recorded.defaults.abi_version == RSF_RENDER_FRONTEND_ABI_VERSION,
        "defaults are stamped");
    check(recorded.log == record_log, "log callback handed over");
    check(recorded.accept_window && recorded.settings_changed, "window filter and settings callback present");

    HWND engine = window_with_class(L"_XRAY_1.5");
    HWND other = window_with_class(L"STATIC_NOT_THE_ENGINE");
    check(engine && recorded.accept_window(nullptr, engine) == 1, "the engine window class is accepted");
    check(other && recorded.accept_window(nullptr, other) == 0, "any other window is refused");
    check(recorded.accept_window(nullptr, nullptr) == 0, "null window refused");
    DestroyWindow(engine); DestroyWindow(other);

    capture_result_bits = RSF_OVERLAY_CAPTURE_MOUSE | RSF_OVERLAY_CAPTURE_KEYBOARD; capture_result = RSF_OK;
    check(query_capture() == 3u, "capture bits reach the input gate");
    capture_result = RSF_ERROR_NOT_READY;
    check(query_capture() == 0u, "a failed capture query never captures");
    capture_result = RSF_OK;

    check(start(options, &reason) == StartStatus::already_started && configure_calls == 1, "a second start does not configure again");
}
}

int main() {
    binding_refuses_cleanly();
    start_refuses_without_the_runtime_api();
    start_hands_over_what_the_adapter_can_apply();
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::puts("Anomaly overlay API binding: PASS");
    return 0;
}
