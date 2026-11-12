// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <cstdio>
#include <cstring>

namespace {
int failures{};
void check(bool ok, const char* message) {
    if (!ok) { ++failures; std::fprintf(stderr, "%s\n", message); }
}
}
int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 2;
    HMODULE module = LoadLibraryExW(argv[1], nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) { std::fprintf(stderr, "Anomaly adapter DLL missing: error=%lu\n", GetLastError()); return 1; }
    const auto entry = reinterpret_cast<rsf_get_game_plugin_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, RSF_GAME_ENTRY_POINT)));
    if (!entry) { std::fprintf(stderr, "Anomaly adapter entry missing\n"); return 1; }
    check(entry(RSF_GAME_ABI_VERSION, nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null API must refuse");
    rsf_game_plugin_api api{};
    check(entry(RSF_GAME_ABI_VERSION, &api) == RSF_ERROR_INVALID_ARGUMENT, "Short API must refuse");
    api.struct_size = sizeof(api);
    check(entry(RSF_GAME_ABI_VERSION + 1, &api) == RSF_ERROR_ABI_MISMATCH, "Wrong ABI must refuse");
    check(entry(RSF_GAME_ABI_VERSION, &api) == RSF_OK, "Current ABI must load");
    if (!api.detect) return 1;
    check(api.info.id && !std::strcmp(api.info.id, "anomaly"), "Adapter identity must be anomaly");
    check(!api.info.rendering_ready, "Recognition must not assert game-tested rendering readiness");
    rsf_game_probe probe{sizeof(probe), 0x8664, 0, "AnomalyDX11.exe",
        "c857b6f49d32c62245611b26106862a773185533856e8b858a7d7e849ed2fb4e"};
    check(api.detect(&probe) == RSF_GAME_RECOGNIZED, "Exact vanilla DX11 build must match");
    probe.executable_name_utf8 = "AnomalyDX11AVX.exe";
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Names and digests must not cross-match");
    probe.sha256_hex = "2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12";
    check(api.detect(&probe) == RSF_GAME_RECOGNIZED, "Exact vanilla AVX build must match");
    probe.executable_name_utf8 = "AnomalyDX11.exe";
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "The AVX digest must not match the standard name");
    probe.executable_name_utf8 = "ANOMALYDX11AVX.EXE";
    probe.sha256_hex = "2032ACFC041E215508692EF4079EF504C7BBABF1793D5668084099F734B92F12";
    check(api.detect(&probe) == RSF_GAME_RECOGNIZED, "Exact AVX build matches regardless of case");
    probe.executable_name_utf8 = "AnomalyDX10AVX.exe";
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Unprofiled DX10 AVX renderer must refuse");
    probe.sha256_hex = "2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f12";
    probe.executable_name_utf8 = "VerifiedDX11.exe";
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Unmapped debug build must refuse");
    probe.executable_name_utf8 = "AnomalyDX11AVX.exe";
    probe.pe_machine = 0x14c;
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "x86 must refuse");
    probe.pe_machine = 0x8664;
    probe.struct_size = sizeof(probe) - 1;
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Short probe must refuse");
    probe.struct_size = sizeof(probe);
    probe.sha256_hex = "2032acfc041e215508692ef4079ef504c7bbabf1793d5668084099f734b92f1z";
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Malformed digest must refuse");
    probe.sha256_hex = nullptr;
    check(api.detect(&probe) == RSF_GAME_UNKNOWN, "Null digest must refuse");
    check(api.detect(nullptr) == RSF_GAME_UNKNOWN, "Null probe must refuse");
    std::printf("Anomaly exact-build contract: %s\n", failures ? "FAILED" : "PASS");
    FreeLibrary(module);
    return failures ? 1 : 0;
}
