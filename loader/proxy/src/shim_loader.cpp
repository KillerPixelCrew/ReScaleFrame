// SPDX-License-Identifier: GPL-3.0-only
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
DWORD WINAPI startup(void*) noexcept try
{
    wchar_t file[32768]{};
    const DWORD length = GetModuleFileNameW(self, file, 32768);
    if (!length || length >= 32768) return 1;
    std::wstring root(file, length); root.resize(root.find_last_of(L"/\\") + 1);
    const auto mod = root + L"ReScaleFrame\\";
    const auto configuration = root + L"ReScaleFrame.ini";
    const auto log_path = mod + L"loader.log";
    auto note = [&](const char* message, DWORD result) {
        if (FILE* stream = _wfopen(log_path.c_str(), L"ab")) {
            std::fprintf(stream, "%s: %lu\n", message, result); std::fclose(stream);
        }
    };
    if (!GetPrivateProfileIntW(L"Loader", L"AutoStart", 1, configuration.c_str())) return 0;
    wchar_t entry_path[32768]{}, entry_name[128]{};
    GetPrivateProfileStringW(L"Loader", L"EntryModule", L"", entry_path, 32768, configuration.c_str());
    GetPrivateProfileStringW(L"Loader", L"EntryPoint", L"", entry_name, 128, configuration.c_str());
    if (!entry_path[0] || !entry_name[0]) return 0;
    std::wstring integration(entry_path);
    if (!(integration.size() > 1 && (integration[1] == L':' || integration[0] == L'\\'))) integration = root + integration;
    const auto runtime_path = mod + L"ReScaleFrame.Runtime.dll";
    const HMODULE runtime = LoadLibraryExW(runtime_path.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!runtime) { note("Runtime DLL load refused", GetLastError()); return 1; }
    const HMODULE adapter = LoadLibraryExW(integration.c_str(), nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!adapter) { note("Integration DLL load refused", GetLastError()); return 1; }
    char entry[384]{};
    if (!WideCharToMultiByte(CP_UTF8, 0, entry_name, -1, entry, sizeof(entry), nullptr, nullptr)) return 1;
    using Start = uint32_t (__stdcall*)(const wchar_t*);
    const auto start = reinterpret_cast<Start>(reinterpret_cast<void*>(GetProcAddress(adapter, entry)));
    if (!start) { note("Integration entry point missing", GetLastError()); return 1; }
    const auto result = start(configuration.c_str());
    note("Integration startup result", result);
    // Runtime, adapter and any callbacks stay loaded for the process lifetime.
    return result;
} catch (...) { return 1; }
}

extern "C" FARPROC rsf_version_resolve(uint32_t index)
{
    if (index >= 17 || !InitOnceExecuteOnce(&initialized, initialize, nullptr, nullptr) || !exports[index]) {
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
