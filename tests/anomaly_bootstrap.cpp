// SPDX-License-Identifier: GPL-3.0-only
#include <windows.h>
#include <cstdio>
#include <filesystem>

int wmain(int argc, wchar_t** argv) {
    if (argc != 3) return 2;
    auto module = LoadLibraryExW(argv[1], nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) return 1;
    using Entry = uint32_t(__stdcall*)(const wchar_t*);
    auto start = reinterpret_cast<Entry>(reinterpret_cast<void*>(GetProcAddress(module, "rsf_anomaly_start")));
    if (!start) { std::fprintf(stderr, "Anomaly cold-start entry missing\n"); return 1; }
    if (!start(nullptr) || !start(L"") || !start(L"C:\\RSF-nonexistent-test-config.ini")) return 1;
    std::filesystem::create_directories(argv[2]);
    const auto ini = std::filesystem::path(argv[2]) / L"anomaly-test.ini";
    auto* file = _wfopen(ini.c_str(), L"wb");
    if (!file) return 1;
    std::fputs("[Anomaly]\nMode=Observe\n", file); std::fclose(file);
    if (GetModuleHandleW(L"ReScaleFrame.Runtime.dll")) return 1;
    const auto result = start(ini.c_str());
    if (result != 2 || GetModuleHandleW(L"ReScaleFrame.Runtime.dll")) {
        std::fprintf(stderr, "Foreign process must refuse before runtime loads, result=%u\n", result); return 1;
    }
    FreeLibrary(module);
    std::puts("Anomaly bootstrap: PASS (no runtime or native hook side effects in foreign host)");
    return 0;
}
