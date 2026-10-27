// SPDX-License-Identifier: GPL-3.0-only
// Opt-in ASI entry for GTA V Enhanced. Optional census forwards native calls;
// this entry does not alter graphics, launcher, anti-cheat, settings or caches.
#include <rescaleframe/plugin_session.h>
#include <rescaleframe/sha256_file.h>
#ifdef RSF_HAVE_VENDOR_CENSUS
#include <rescaleframe/vendor_census.h>
#endif
#include "build_identity.h"
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
    std::lock_guard<std::mutex> lock(log_guard);
    if (FILE* file = _wfopen(log_path.c_str(), L"ab")) {
        std::fprintf(file, "[%llu] %s\n", static_cast<unsigned long long>(GetTickCount64()), message);
        std::fclose(file);
    }
} catch (...) {}

DWORD WINAPI startup(void*) noexcept try
{
    wchar_t path[32768]{};
    const auto length = GetModuleFileNameW(self, path, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(path, length);
    const auto separator = root.find_last_of(L"/\\");
    if (separator == std::wstring::npos) return 1;
    root.resize(separator + 1);
    const auto config = root + L"ReScaleFrame.ini";
    if (!GetPrivateProfileIntW(L"GTA5Enhanced", L"Enable", 0, config.c_str())) return 0;
    const auto mod = root + L"ReScaleFrame\\";
    if (!CreateDirectoryW(mod.c_str(), nullptr) && GetLastError() != ERROR_ALREADY_EXISTS) return 1;
    log_path = mod + L"gta5-enhanced.log";
    log(nullptr, "GTA V Enhanced entry started; RSF rendering is disabled.");

    const auto executable_length = GetModuleFileNameW(nullptr, path, 32768);
    if (!executable_length || executable_length >= 32768) return 1;
    std::string digest;
    if (!rsf::sha256_file(path, digest)) { log(nullptr, "Executable fingerprint could not be read."); return 1; }
#ifdef RSF_HAVE_VENDOR_CENSUS
    if (GetPrivateProfileIntW(L"GTA5Enhanced", L"Census", 0, config.c_str())) {
        if (digest != rsf_gta5_enhanced::executable_sha256) {
            log(nullptr, "Vendor census refused: unknown executable fingerprint.");
        } else {
            const auto directory = mod + L"gta5-enhanced-census\\" +
                std::to_wstring(GetCurrentProcessId()) + L"-" + std::to_wstring(GetTickCount64());
            // The census copies this path during installation. Start on this worker
            // after fingerprinting, before loading RSF's runtime or vendor modules.
            const int size = WideCharToMultiByte(CP_UTF8, 0, directory.c_str(), -1, nullptr, 0, nullptr, nullptr);
            std::string utf8(size > 0 ? static_cast<size_t>(size) : 0, '\0');
            if (size > 0 && WideCharToMultiByte(CP_UTF8, 0, directory.c_str(), -1,
                    utf8.data(), size, nullptr, nullptr)) {
                rsf_census_options census{};
                census.struct_size = sizeof(census); census.abi_version = RSF_CENSUS_ABI_VERSION;
                census.output_dir_utf8 = utf8.c_str();
                // The existing probe bounds its output to 64 MiB, samples after
                // 32 full records per site, and periodically flushes its summary.
                // Skip signatures for other games and the unselected Vulkan path.
                census.flags = RSF_CENSUS_FLAG_NO_STATIC_SCAN | RSF_CENSUS_FLAG_NO_VULKAN;
                const auto result = rsf_census_install(&census);
                char message[128]{};
                std::snprintf(message, sizeof(message), "Vendor census install result=%d flags=0x%x",
                    int(result), census.flags);
                log(nullptr, message);
            } else log(nullptr, "Vendor census refused: invalid output path.");
        }
    }
#endif
    const auto runtime_path = mod + L"ReScaleFrame.Runtime.dll";
    const auto runtime = LoadLibraryExW(runtime_path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) { log(nullptr, "Runtime DLL load refused."); return 1; }
    const auto prepare = reinterpret_cast<decltype(&rsf_plugin_session_prepare)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_prepare")));
    const auto start = reinterpret_cast<decltype(&rsf_plugin_session_start)>(
        reinterpret_cast<void*>(GetProcAddress(runtime, "rsf_plugin_session_start")));
    if (!prepare || !start) { log(nullptr, "Runtime plugin lifecycle exports are unavailable."); FreeLibrary(runtime); return 1; }
    const rsf_game_probe probe{sizeof(probe), 0x8664, 0, "GTA5_Enhanced.exe", digest.c_str()};
    const auto plugin = mod + L"ReScaleFrame.Game.GTA5Enhanced.dll";
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options); options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = plugin.c_str(); options.probe = &probe;
    options.services.struct_size = sizeof(options.services); options.services.abi_version = RSF_GAME_ABI_VERSION;
    options.services.session_id = GetTickCount64() + 1; options.services.log = log;
    const auto prepared = prepare(&options, &session);
    char message[192]{};
    std::snprintf(message, sizeof(message), "GTA V Enhanced prepare result=%d sha256=%s", int(prepared), digest.c_str());
    log(nullptr, message);
    if (prepared != RSF_OK) {
        if (!session) FreeLibrary(runtime); // BUSY may return a retained owner.
        return 1;
    }
    const auto started = start(session);
    std::snprintf(message, sizeof(message), "GTA V Enhanced start result=%d.", int(started));
    log(nullptr, message);
    // Retain the session and runtime until process termination, including a failed start.
    return started == RSF_OK ? 0 : 1;
} catch (...) { return 1; }
}

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID)
{
    if (reason == DLL_PROCESS_ATTACH) {
        DisableThreadLibraryCalls(instance);
        // Census uses static TLS, so DisableThreadLibraryCalls can fail. Keep
        // the entry's stack small even for DLL_THREAD_ATTACH on 64 KiB threads:
        // an automatic path buffer is reserved by the prologue for every reason.
        static wchar_t executable[32768]{};
        const auto length = GetModuleFileNameW(nullptr, executable, 32768);
        const auto* leaf = length && length < 32768 ? wcsrchr(executable, L'\\') : nullptr;
        if (leaf && !_wcsicmp(leaf + 1, L"GTA5_Enhanced.exe")) {
            self = instance;
            if (HANDLE thread = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr)) CloseHandle(thread);
        }
    }
    return TRUE;
}
