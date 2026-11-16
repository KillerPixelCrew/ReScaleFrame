// SPDX-License-Identifier: MIT
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/sha256_file.h>
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <mutex>
#include <string>
#include <atomic>

namespace {
HMODULE self = nullptr;
INIT_ONCE forwarding = INIT_ONCE_STATIC_INIT;
FARPROC exports[12]{};
constexpr WORD ordinals[] = {1,2,3,4,5,6,7,8,100,101,102,103};
std::wstring log_path;
std::mutex log_mutex;
rsf_plugin_session* session = nullptr;
int (*cpu_sink)(const rsf_game_cpu_event*) = nullptr;
std::atomic<uint32_t> cpu_samples{};
void log(void*, const char*);
void cpu_event(void*, const rsf_game_cpu_event* event)
{
    const int accepted = cpu_sink ? cpu_sink(event) : 0;
    if (event && cpu_samples.fetch_add(1, std::memory_order_relaxed) < 32) {
        char message[192]{};
        std::snprintf(message, sizeof(message), "Runtime CPU fact stage=%u source=%llu thread=%lu accepted=%d flags=0x%x",
            event->stage, static_cast<unsigned long long>(event->source_frame_id), GetCurrentThreadId(), accepted, event->flags);
        log(nullptr, message);
    }
}

BOOL CALLBACK initialize(PINIT_ONCE, void*, void**)
{
    wchar_t directory[MAX_PATH]{};
    const auto length = GetSystemDirectoryW(directory, MAX_PATH);
    if (!length || length >= MAX_PATH) return FALSE;
    const auto path = std::wstring(directory, length) + L"\\xinput1_3.dll";
    const auto system = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!system) return FALSE;
    for (size_t i = 0; i < 12; ++i) exports[i] = GetProcAddress(system, MAKEINTRESOURCEA(ordinals[i]));
    return TRUE;
}

void log(void*, const char* message)
{
    std::lock_guard<std::mutex> lock(log_mutex);
    if (auto* file = _wfopen(log_path.c_str(), L"ab")) {
        std::fprintf(file, "%s\n", message ? message : "");
        std::fclose(file);
    }
}

DWORD WINAPI startup(void*) noexcept try
{
    wchar_t module_path[32768]{};
    const auto length = GetModuleFileNameW(self, module_path, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(module_path, length);
    root.resize(root.find_last_of(L"/\\") + 1);
    const auto directory = root + L"ReScaleFrame\\";
    CreateDirectoryW(directory.c_str(), nullptr);
    log_path = directory + L"loader.log";
    const auto runtime_path = directory + L"ReScaleFrame.Runtime.dll";
    const auto plugin_path = directory + L"ReScaleFrame.Entry.dll";
    const auto runtime = LoadLibraryExW(runtime_path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) { log(nullptr, "Runtime module load refused."); return 1; }
    const auto prepare = reinterpret_cast<decltype(&rsf_plugin_session_prepare)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_prepare")));
    const auto start = reinterpret_cast<decltype(&rsf_plugin_session_start)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_start")));
    if (!prepare || !start) { log(nullptr, "Runtime session ABI unavailable."); return 1; }
    cpu_sink = reinterpret_cast<decltype(cpu_sink)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_native_cpu_event")));
    wchar_t executable[32768]{};
    const auto executable_length = GetModuleFileNameW(nullptr, executable, 32768);
    if (!executable_length || executable_length >= 32768) return 1;
    const auto* leaf = std::wcsrchr(executable, L'\\');
    char name[1024]{};
    if (!WideCharToMultiByte(CP_UTF8, 0, leaf ? leaf + 1 : executable, -1,
                             name, sizeof(name), nullptr, nullptr)) return 1;
    std::string digest;
    if (!rsf::sha256_file(executable, digest)) return 1;
    rsf_game_probe probe{};
    probe.struct_size = sizeof(probe);
    probe.executable_name_utf8 = name;
    probe.sha256_hex = digest.c_str();
    probe.pe_machine = 0x8664;
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin_path.c_str();
    options.probe = &probe;
    options.services.struct_size = sizeof(options.services);
    options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = 1;
    options.services.log = log;
    options.services.cpu_event = cpu_sink ? cpu_event : nullptr;
    const auto prepared = prepare(&options, &session);
    char message[128]{};
    std::snprintf(message, sizeof(message), "Plugin prepare result=%u", unsigned(prepared));
    log(nullptr, message);
    if (prepared != RSF_OK) return unsigned(prepared);
    const auto started = start(session);
    std::snprintf(message, sizeof(message), "Plugin start result=%u", unsigned(started));
    log(nullptr, message);
    // Runtime owns the session. Carrier callbacks remain valid until process exit.
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

BOOL WINAPI DllMain(HINSTANCE instance, DWORD event, void*)
{
    if (event == DLL_PROCESS_ATTACH) {
        self = instance;
        DisableThreadLibraryCalls(instance);
        const auto thread = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr);
        if (thread) CloseHandle(thread);
    }
    return TRUE;
}
