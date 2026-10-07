// SPDX-License-Identifier: GPL-3.0-only
// Generic version.dll carrier. Windows exports resolve lazily and independently of mod startup.
// DllMain selects the AC7 carrier or starts a Unity worker; DLL loading and Mono waits occur in
// that worker. The system version module and loaded runtime remain for the process lifetime.
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <string>
#include <cwchar>

extern "C" BOOL WINAPI rsf_ac7_proxy_process_event(HINSTANCE, DWORD, LPVOID);

namespace {
HMODULE self = nullptr;
HMODULE system_version = nullptr;
INIT_ONCE initialized = INIT_ONCE_STATIC_INIT;
FARPROC exports[17]{};
constexpr const char* names[] = {
    "GetFileVersionInfoA", "GetFileVersionInfoByHandle", "GetFileVersionInfoExA", "GetFileVersionInfoExW",
    "GetFileVersionInfoSizeA", "GetFileVersionInfoSizeExA", "GetFileVersionInfoSizeExW", "GetFileVersionInfoSizeW",
    "GetFileVersionInfoW", "VerFindFileA", "VerFindFileW", "VerInstallFileA", "VerInstallFileW",
    "VerLanguageNameA", "VerLanguageNameW", "VerQueryValueA", "VerQueryValueW"};
// Indexed by version_forwarders.asm, not shim_exports.def ordinals. InitOnce serializes lazy
// loading from System32 so a version.dll proxy cannot recursively load itself by basename.
BOOL CALLBACK initialize(PINIT_ONCE, void*, void**)
{
    wchar_t directory[MAX_PATH]{};
    const UINT length = GetSystemDirectoryW(directory, MAX_PATH);
    if (!length || length >= MAX_PATH) return FALSE;
    std::wstring path(directory, length); path += L"\\version.dll";
    system_version = LoadLibraryExW(path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!system_version) return FALSE;
    for (size_t i = 0; i < 17; ++i) exports[i] = GetProcAddress(system_version, names[i]);
    return TRUE;
}
// An existing ReScaleFrame.ini enables mod startup; [UnitySR] AutoStart defaults to 1.
// Install generation interception before the first chain, then wait up to 90 seconds for Mono
// and retry SR startup while its entry returns 10 (not ready). Returned status is diagnostic;
// Windows API forwarding remains usable even when this worker refuses or catches an exception.
DWORD WINAPI startup(void*) noexcept try
{
    wchar_t file[32768]{};
    const DWORD length = GetModuleFileNameW(self, file, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(file, length); root.resize(root.find_last_of(L"/\\") + 1);
    const std::wstring mod = root + L"ReScaleFrame\\";
    const std::wstring runtime = mod + L"ReScaleFrame.Runtime.dll";
    const std::wstring configuration = root + L"ReScaleFrame.ini";
    const std::wstring log_path = mod + L"loader.log";
    auto note = [&](const char* message, DWORD result) {
        FILE* stream = _wfopen(log_path.c_str(), L"ab");
        if (stream) { std::fprintf(stream, "%s: %lu\n", message, result); std::fclose(stream); }
    };
    if (GetFileAttributesW(configuration.c_str()) == INVALID_FILE_ATTRIBUTES) return 0;
    if (!GetPrivateProfileIntW(L"UnitySR", L"AutoStart", 1, configuration.c_str())) return 0;
    note("Version shim loaded; Windows API forwarding is independent of mod startup", 0);
    // Vendor generation must own the first chain. Install its runtime interception before
    // waiting for Mono; the worker still performs all loading outside DllMain's loader lock.
    HMODULE module = nullptr;
    {
        module = LoadLibraryExW(runtime.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
        if (!module) { note("Early generation runtime load refused", GetLastError()); return 1; }
        using FgStart = uint32_t (__stdcall*)(const wchar_t*);
        auto fg_start = reinterpret_cast<FgStart>(reinterpret_cast<void*>(GetProcAddress(module, "rsf_unity_fg_start")));
        note("Cold generation startup result", fg_start ? fg_start(configuration.c_str()) : 1);
    }
    // Unity initializes its scripting child domain after native libraries load. Nothing waits
    // under the loader lock, and no graphics device is created by this worker.
    const ULONGLONG deadline = GetTickCount64() + 90000;
    while (GetTickCount64() < deadline && !GetModuleHandleW(L"mono-2.0-bdwgc.dll")) Sleep(100);
    if (!GetModuleHandleW(L"mono-2.0-bdwgc.dll")) { note("Mono runtime did not appear", 1); return 1; }
    note("Mono runtime appeared", GetCurrentProcessId());
    Sleep(2000);
    note("Loading RSF runtime", GetCurrentProcessId());
    if (!module) module = LoadLibraryExW(runtime.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) { note("Runtime DLL load refused", GetLastError()); return 1; }
    note("RSF runtime loaded", GetCurrentProcessId());
    using Start = uint32_t (__stdcall*)(const wchar_t*);
    auto start = reinterpret_cast<Start>(reinterpret_cast<void*>(GetProcAddress(module, "rsf_unity_sr_start")));
    if (!start) { note("Runtime entry point missing", GetLastError()); return 1; }
    DWORD result = 10;
    while (GetTickCount64() < deadline) {
        note("Preparing Unity plugin", GetCurrentProcessId());
        result = start(configuration.c_str());
        if (result != 10) break;
        Sleep(500);
    }
    note("Unity SR startup result", result);
    // The runtime and its callback code remain loaded for the game process lifetime.
    return result;
}
catch (...) { return 1; }
}

// Called by assembly forwarding stubs. The resolver deliberately raises a noncontinuable
// exception for an unavailable export because generic wrappers cannot synthesize its return ABI.
extern "C" FARPROC rsf_version_resolve(uint32_t index)
{
    if (index >= 17 || !InitOnceExecuteOnce(&initialized, initialize, nullptr, nullptr) || !exports[index]) {
        SetLastError(ERROR_PROC_NOT_FOUND);
        RaiseException(EXCEPTION_ILLEGAL_INSTRUCTION, EXCEPTION_NONCONTINUABLE, 0, nullptr);
        return nullptr;
    }
    return exports[index];
}
// Dispatch AC7 process attachment by executable name; crash reporters need forwarding only.
// Other processes receive a detached Unity worker. No renderer teardown runs under loader lock.
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, void*)
{
    if (reason == DLL_PROCESS_ATTACH) {
        self = instance; DisableThreadLibraryCalls(instance);
        // Unity loads native dependencies on threads with small stacks. Keep path storage in
        // the DLL's data section instead of reserving a 64 KiB DllMain stack frame.
        static wchar_t executable[32768]{};
        if (GetModuleFileNameW(nullptr, executable, 32768)) {
            const wchar_t* leaf = wcsrchr(executable, L'\\');
            if (_wcsicmp(leaf ? leaf+1 : executable, L"Ace7Game.exe") == 0)
                return rsf_ac7_proxy_process_event(instance, reason, nullptr);
            // Unity's crash reporter imports version.dll from this same directory.
            // It needs Windows forwarding, without another renderer/mod startup.
            if (_wcsicmp(leaf ? leaf+1 : executable, L"UnityCrashHandler64.exe") == 0 ||
                _wcsicmp(leaf ? leaf+1 : executable, L"UnityCrashHandler32.exe") == 0) return TRUE;
        }
        HANDLE worker = CreateThread(nullptr, 0, startup, nullptr, 0, nullptr);
        if (worker) CloseHandle(worker);
    }
    return TRUE;
}
