// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <mutex>
#include <string>

namespace {
HMODULE self = nullptr;
INIT_ONCE forwarding = INIT_ONCE_STATIC_INIT;
FARPROC exports[12]{};
constexpr WORD ordinals[] = {1,2,3,4,5,6,7,8,100,101,102,103};
std::wstring log_path;
std::mutex log_guard;
rsf_plugin_session* session = nullptr;

BOOL CALLBACK initialize(PINIT_ONCE, void*, void**)
{
    wchar_t directory[MAX_PATH]{};
    const auto length = GetSystemDirectoryW(directory, MAX_PATH);
    if (!length || length >= MAX_PATH) return FALSE;
    const std::wstring path = std::wstring(directory, length) + L"\\xinput1_3.dll";
    const auto module = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return FALSE;
    for (size_t i = 0; i < 12; ++i) exports[i] = GetProcAddress(module, MAKEINTRESOURCEA(ordinals[i]));
    return TRUE;
}
void log(void*, const char* message) noexcept try
{
    std::lock_guard<std::mutex> lock(log_guard);
    FILE* stream = _wfopen(log_path.c_str(), L"ab");
    if (stream) {
        std::fprintf(stream, "[%llu] %s\n", static_cast<unsigned long long>(GetTickCount64()), message);
        std::fclose(stream);
    }
}
catch (...) {}

DWORD WINAPI startup(void*) noexcept try
{
    wchar_t filename[32768]{};
    const auto length = GetModuleFileNameW(self, filename, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(filename, length);
    root.resize(root.find_last_of(L"/\\") + 1);
    const auto config = root + L"ReScaleFrame.ini";
    if (!GetPrivateProfileIntW(L"ProjectWingman", L"ObserveNative", 0, config.c_str())) return 0;
    const auto mod = root + L"ReScaleFrame\\";
    log_path = mod + L"project-wingman.log";
    log(nullptr, "XInput carrier started; Windows controller forwarding remains independent.");
    const auto runtime_path = mod + L"ReScaleFrame.Runtime.dll";
    const auto runtime = LoadLibraryExW(runtime_path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) { log(nullptr, "Runtime DLL load refused."); return 1; }
    const auto prepare = reinterpret_cast<decltype(&rsf_plugin_session_prepare)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_prepare")));
    const auto start = reinterpret_cast<decltype(&rsf_plugin_session_start)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_start")));
    if (!prepare || !start) { log(nullptr, "Runtime plugin lifecycle exports are unavailable."); return 1; }
    if (!GetModuleFileNameW(nullptr, filename, 32768)) return 1;
    std::string digest;
    if (!rsf::sha256_file(filename, digest)) { log(nullptr, "Executable fingerprint could not be read."); return 1; }
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0,
        "ProjectWingman-Win64-Shipping.exe", digest.c_str()};
    const auto plugin_path = mod + L"ReScaleFrame.Game.ProjectWingman.dll";
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin_path.c_str(); options.probe = &probe;
    options.services.struct_size = sizeof(options.services);
    options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = GetTickCount64() + 1;
    options.services.log = log;
    const auto prepared = prepare(&options, &session);
    char message[160]{};
    std::snprintf(message, sizeof(message), "Project Wingman observer prepare result=%u", unsigned(prepared));
    log(nullptr, message);
    if (prepared != RSF_OK) return unsigned(prepared);
    const auto started = start(session);
    std::snprintf(message, sizeof(message),
        "Project Wingman observer start result=%u; graph facts only, SR/FG/latency not implemented.", unsigned(started));
    log(nullptr, message);
    // Runtime, session and carrier callbacks remain owned until process termination.
    return unsigned(started);
}
catch (...) { return 1; }
}

extern "C" FARPROC rsf_xinput_resolve(uint32_t index)
{
    if (index >= 12 || !InitOnceExecuteOnce(&forwarding, initialize, nullptr, nullptr) || !exports[index]) {
        SetLastError(ERROR_PROC_NOT_FOUND);
        RaiseException(EXCEPTION_ILLEGAL_INSTRUCTION, EXCEPTION_NONCONTINUABLE, 0, nullptr);
        return nullptr;
    }
    return exports[index];
}
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        self = instance; DisableThreadLibraryCalls(instance);
        static wchar_t executable[32768]{};
        if (GetModuleFileNameW(nullptr, executable, 32768)) {
            const auto* leaf = wcsrchr(executable, L'\\');
            if (!_wcsicmp(leaf ? leaf + 1 : executable, L"ProjectWingman-Win64-Shipping.exe")) {
                HANDLE worker = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr);
                if (worker) CloseHandle(worker);
            }
        }
    }
    return TRUE;
}
