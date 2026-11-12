// SPDX-License-Identifier: GPL-3.0-only
#include "native_observer.h"
#include "native_profile.h"
#include "overlay_service.h"
#include "motion_stage.h"
#include "sr_stage.h"
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <mutex>
#include <string>

namespace {
std::mutex entry_guard, log_guard;
std::wstring log_path;
rsf_plugin_session* session{};
bool started{};   // every requested mode is complete
bool observed{};  // Observe is running; a retry after an overlay refusal must not start a second session
void log(void*, const char* message) noexcept try {
    std::lock_guard lock(log_guard);
    if (auto* file = _wfopen(log_path.c_str(), L"ab")) {
        std::fprintf(file, "[%lu:%llu] %s\n", GetCurrentProcessId(),
            static_cast<unsigned long long>(GetTickCount64()), message);
        std::fclose(file);
    }
} catch (...) {}
template<class Function> Function resolve(HMODULE module, const char* name) noexcept {
    return reinterpret_cast<Function>(reinterpret_cast<void*>(GetProcAddress(module, name)));
}
void result_log(const char* operation, rsf_result result) noexcept {
    char text[192]{};
    std::snprintf(text, sizeof(text), "Anomaly %s result=%d", operation, result);
    log(nullptr, text);
}
uint32_t start_observation(HMODULE runtime, const std::string& hash) noexcept try {
    const auto prepare = resolve<decltype(&rsf_plugin_session_prepare)>(runtime, "rsf_plugin_session_prepare");
    const auto start = resolve<decltype(&rsf_plugin_session_start)>(runtime, "rsf_plugin_session_start");
    if (!prepare || !start) { log(nullptr, "Anomaly refused: required Runtime plugin-session exports missing."); return 4; }
    HMODULE self{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        reinterpret_cast<LPCWSTR>(reinterpret_cast<void*>(&start_observation)), &self)) return 4;
    wchar_t plugin[32768]{};
    const auto plugin_length = GetModuleFileNameW(self, plugin, 32768);
    if (!plugin_length || plugin_length >= 32768) return 4;
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0, "AnomalyDX11.exe", hash.c_str()};
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin; options.probe = &probe;
    options.services.struct_size = sizeof(options.services);
    options.services.abi_version = RSF_GAME_ABI_VERSION;
    LARGE_INTEGER qpc{};
    if (!QueryPerformanceCounter(&qpc) || qpc.QuadPart <= 0) return 4;
    options.services.session_id = static_cast<uint64_t>(qpc.QuadPart);
    options.services.log = log;
    if (session) { log(nullptr, "Anomaly refused: an earlier partial session is still owned."); return 5; }
    log(nullptr, "Anomaly Observe cold startup; borrowing existing Runtime, before DirectInput/device creation.");
    const auto prepared = prepare(&options, &session);
    result_log("session prepare", prepared);
    if (prepared != RSF_OK) return 5;
    const auto activated = start(session);
    result_log("session start", activated);
    if (activated != RSF_OK) return 6;
    log(nullptr, "Anomaly Observe active. This is native-boundary instrumentation, not working SR or FG.");
    return 0;
} catch (...) { log(nullptr, "Anomaly startup refused after a contained C++ exception."); return 8; }
}
// Synchronous cold entry called by the thin carrier before genuine DirectInput creation.
// It borrows the one already-loaded Runtime DLL and uses its plugin-session owner.
extern "C" __declspec(dllexport) uint32_t __stdcall rsf_anomaly_start(const wchar_t* configuration) noexcept try {
    if (!configuration || !*configuration) return 1;
    const auto attributes = GetFileAttributesW(configuration);
    if (attributes == INVALID_FILE_ATTRIBUTES || (attributes & FILE_ATTRIBUTE_DIRECTORY)) return 1;
    const auto config = std::filesystem::path(configuration);
    if (!config.is_absolute()) return 1;
    std::unique_lock lock(entry_guard);
    if (started) return 0;
    wchar_t executable[32768]{};
    const auto length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!length || length >= 32768) return 2;
    const auto* separator = std::wcsrchr(executable, L'\\');
    const auto* name = separator ? separator + 1 : executable;
    if (_wcsicmp(name, L"AnomalyDX11.exe")) return 2;
    std::string hash;
    if (!rsf::sha256_file(executable, hash) || hash != rsf::anomaly::dx11_hash) return 2;
    log_path = (config.parent_path() / L"ReScaleFrame.Anomaly.log").wstring();
    wchar_t mode[64]{};
    GetPrivateProfileStringW(L"Anomaly", L"Mode", L"", mode, 64, configuration);
    const bool observe = !std::wcscmp(mode, L"Observe");
    // Overlay=1 is independent of Mode: it wires only the shared overlay, never SR or FG.
    const bool overlay = GetPrivateProfileIntW(L"Anomaly", L"Overlay", 0, configuration) != 0;
    if (!observe && !overlay) {
        log(nullptr, "Anomaly startup refused: only explicitly selected Observe mode or Overlay=1 is currently validated; SR/FG is not implemented.");
        return 7;
    }
    HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime) { log(nullptr, "Anomaly refused: carrier has not loaded the existing Runtime DLL."); return 3; }
    // SR Stage 1 (SRStage=1 / SRStageCapture=1) rides on Observe: the observer's start() installs it after the
    // geometry hooks. Inert when neither key is set.
    if (const auto sr_options = rsf::anomaly::sr::read_options(configuration); sr_options.selected()) {
        if (!observe) log(nullptr, "Anomaly SR stage refused: SRStage/SRStageCapture require Mode=Observe; game left stock.");
        else if (!observed) rsf::anomaly::sr::configure(sr_options, log, nullptr);
    }
    // Motion MRT (Motion=1) also rides on Observe; the observer installs it after SR. Inert when Motion is not 1.
    if (const auto motion_options = rsf::anomaly::motion::read_options(configuration); motion_options.enabled) {
        if (!observe) log(nullptr, "Anomaly motion refused: Motion=1 requires Mode=Observe; shaders stay stock.");
        else if (!observed) rsf::anomaly::motion::configure(motion_options, log, nullptr);
    }
    if (observe && !observed) {
        const auto observe_status = start_observation(runtime, hash);
        if (observe_status) return observe_status;
        observed = true;
    }
    if (overlay) {
        rsf::anomaly::overlay::StartOptions options{};
        options.runtime = runtime; options.configuration = configuration; options.log = log;
        const char* reason = nullptr;
        const auto overlay_status = rsf::anomaly::overlay::start(options, &reason);
        if (overlay_status != rsf::anomaly::overlay::StartStatus::started &&
            overlay_status != rsf::anomaly::overlay::StartStatus::already_started)
            return 9;
    }
    started = true;
    return 0;
} catch (...) { log(nullptr, "Anomaly startup refused after a contained C++ exception."); return 8; }
