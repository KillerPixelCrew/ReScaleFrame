// SPDX-License-Identifier: GPL-3.0-only
#include <windows.h>
#include <cstdio>
#include <cwchar>
#include <filesystem>
#include <string>

namespace {
using Entry = uint32_t(__stdcall*)(const wchar_t*);
// Loads the adapter and calls its cold entry with an Observe configuration. The process must be refused (status 2) before
// the Runtime DLL loads: either its name is not a profiled executable, or it carries a profiled name (AnomalyDX11.exe,
// AnomalyDX11AVX.exe) without that build's SHA-256.
int refused_observe(const wchar_t* dll, const std::filesystem::path& directory, bool null_checks) {
    auto module = LoadLibraryExW(dll, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) return 1;
    auto start = reinterpret_cast<Entry>(reinterpret_cast<void*>(GetProcAddress(module, "rsf_anomaly_start")));
    if (!start) { std::fprintf(stderr, "Anomaly cold-start entry missing\n"); return 1; }
    if (null_checks && (!start(nullptr) || !start(L"") || !start(L"C:\\RSF-nonexistent-test-config.ini"))) return 1;
    std::filesystem::create_directories(directory);
    const auto ini = directory / L"anomaly-test.ini";
    auto* file = _wfopen(ini.c_str(), L"wb");
    if (!file) return 1;
    std::fputs("[Anomaly]\nMode=Observe\n", file); std::fclose(file);
    if (GetModuleHandleW(L"ReScaleFrame.Runtime.dll")) return 1;
    const auto result = start(ini.c_str());
    if (result != 2 || GetModuleHandleW(L"ReScaleFrame.Runtime.dll")) {
        std::fprintf(stderr, "Unprofiled process must refuse before runtime loads, result=%u\n", result); return 1;
    }
    FreeLibrary(module);
    return 0;
}
// Runs a copy of this test under a profiled executable name; the copy's hash is not that build's, so it must refuse.
int named_copy(const wchar_t* dll, const std::filesystem::path& directory, const wchar_t* name) {
    wchar_t self[32768]{};
    const auto length = GetModuleFileNameW(nullptr, self, 32768);
    if (!length || length >= 32768) return 1;
    const auto folder = directory / L"named";
    std::filesystem::create_directories(folder);
    const auto copy = folder / name;
    if (!CopyFileW(self, copy.c_str(), FALSE)) { std::fprintf(stderr, "copy failed: %lu\n", GetLastError()); return 1; }
    std::wstring command = L"\"" + copy.wstring() + L"\" --as-named-build \"" + std::wstring(dll) + L"\" \"" +
        (folder / L"child").wstring() + L"\"";
    STARTUPINFOW startup{sizeof(startup)};
    PROCESS_INFORMATION process{};
    if (!CreateProcessW(copy.c_str(), command.data(), nullptr, nullptr, FALSE, 0, nullptr, nullptr, &startup, &process)) return 1;
    WaitForSingleObject(process.hProcess, 60000);
    DWORD code = 1;
    GetExitCodeProcess(process.hProcess, &code);
    CloseHandle(process.hThread);
    CloseHandle(process.hProcess);
    DeleteFileW(copy.c_str());
    if (code) std::fprintf(stderr, "%ls with a foreign hash was not refused (exit %lu)\n", name, code);
    return code ? 1 : 0;
}
}

int wmain(int argc, wchar_t** argv) {
    if (argc == 4 && !std::wcscmp(argv[1], L"--as-named-build")) return refused_observe(argv[2], argv[3], false);
    if (argc != 3) return 2;
    if (refused_observe(argv[1], argv[2], true)) return 1;
    for (const auto* name : {L"AnomalyDX11.exe", L"AnomalyDX11AVX.exe"})
        if (named_copy(argv[1], argv[2], name)) return 1;
    std::puts("Anomaly bootstrap: PASS (foreign host and profiled names with foreign hashes refuse; no runtime or native "
              "hook side effects)");
    return 0;
}
