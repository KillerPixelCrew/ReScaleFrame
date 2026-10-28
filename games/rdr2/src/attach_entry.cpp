// SPDX-License-Identifier: GPL-3.0-only
// Research-only late attachment after a guarded startup refusal. Load this helper by its
// absolute staging path; never replace files in a running installation or load another runtime.
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/render_host.h>
#include "runtime_setup.h"
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <mutex>
#include <string>

namespace {
HMODULE self{};
std::wstring log_path;
std::mutex log_guard;
rsf_plugin_session* session{};
void log(void*, const char* message) noexcept try
{
    std::lock_guard lock(log_guard);
    if (auto* file = _wfopen(log_path.c_str(), L"ab")) {
        std::fprintf(file, "[%llu] %s\n", static_cast<unsigned long long>(GetTickCount64()), message);
        std::fclose(file);
    }
} catch (...) {}
DWORD WINAPI attach(void*) noexcept try
{
    wchar_t path[32768]{};
    if (!GetModuleFileNameW(self, path, 32768)) return 1;
    auto* separator = std::wcsrchr(path, L'\\');
    if (!separator) return 1;
    *++separator = L'\0';
    const auto configuration = std::wstring(path) + L"attach.ini";
    wchar_t ini[32768]{}, plugin[32768]{}, directory[32768]{}, output[32768]{};
    GetPrivateProfileStringW(L"Attach", L"Configuration", L"", ini, 32768, configuration.c_str());
    GetPrivateProfileStringW(L"Attach", L"Plugin", L"", plugin, 32768, configuration.c_str());
    GetPrivateProfileStringW(L"Attach", L"RuntimeDirectory", L"", directory, 32768, configuration.c_str());
    GetPrivateProfileStringW(L"Attach", L"Log", L"", output, 32768, configuration.c_str());
    if (!ini[0] || !plugin[0] || !directory[0] || !output[0]) return 1;
    log_path = output;
    log(nullptr, "RDR2 research attachment started; borrowing the already loaded runtime.");
    HMODULE runtime = GetModuleHandleW(L"ReScaleFrame.Runtime.dll");
    if (!runtime) { log(nullptr, "Attachment refused: no existing runtime."); return 1; }
    auto prepare = reinterpret_cast<decltype(&rsf_plugin_session_prepare)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_prepare")));
    auto start = reinterpret_cast<decltype(&rsf_plugin_session_start)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_start")));
    auto create = reinterpret_cast<decltype(&rsf_render_host_create)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_render_host_create")));
    if (!prepare || !start || !create || !GetModuleFileNameW(nullptr, path, 32768)) return 1;
    std::string digest;
    if (!rsf::sha256_file(path, digest)) return 1;
    const auto* name = std::wcsrchr(path, L'\\');
    if (!name || _wcsicmp(name + 1, L"RDR2.exe")) return 1;
    rsf_game_probe probe{sizeof(probe), 0x8664, 0, "RDR2.exe", digest.c_str()};
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin; options.probe = &probe;
    options.services.struct_size = sizeof(options.services); options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = GetTickCount64() + 1; options.services.log = log;
    const auto setup = rsf_rdr2_runtime_setup(ini, directory);
    auto result = create(&setup, &options.services);
    if (result == RSF_OK) result = prepare(&options, &session);
    char message[160]{};
    std::snprintf(message, sizeof(message), "RDR2 research prepare result=%d sha256=%s", result, digest.c_str());
    log(nullptr, message);
    if (result == RSF_OK) {
        result = start(session);
        std::snprintf(message, sizeof(message), "RDR2 research start result=%d", result);
        log(nullptr, message);
    }
    return result == RSF_OK ? 0u : 1u;
} catch (...) { log(nullptr, "RDR2 research attachment failed with an exception."); return 1; }
}
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        self = instance; DisableThreadLibraryCalls(instance);
        if (auto thread = CreateThread(nullptr, 0, attach, nullptr, 0, nullptr)) CloseHandle(thread);
    }
    return TRUE;
}
