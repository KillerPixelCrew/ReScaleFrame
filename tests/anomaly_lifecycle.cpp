// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/game_api.h>
#include <windows.h>
#include <cstdio>

int wmain(int argc, wchar_t** argv) {
    if (argc != 2) return 2;
    auto module = LoadLibraryExW(argv[1], nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!module) return 1;
    auto entry = reinterpret_cast<rsf_get_game_plugin_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, RSF_GAME_ENTRY_POINT)));
    rsf_game_plugin_api api{}; api.struct_size = sizeof(api);
    if (!entry || entry(RSF_GAME_ABI_VERSION, &api) != RSF_OK) return 1;
    const auto& h = api.hooks;
    if (!h.prepare || !h.start || !h.quiesce || !h.stop || !h.status) {
        std::fprintf(stderr, "Anomaly lifecycle callbacks missing\n"); return 1;
    }
    int failures{};
    auto expect = [&](bool ok, const char* message) {
        if (!ok) { ++failures; std::fprintf(stderr, "%s\n", message); }
    };
    expect(h.prepare(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null prepare must refuse");
    expect(h.start(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null start must refuse");
    expect(h.status(nullptr) == RSF_ERROR_INVALID_ARGUMENT, "Null status must refuse");
    rsf_game_renderer_status status{sizeof(status), RSF_GAME_ABI_VERSION};
    expect(h.status(&status) == RSF_OK && !status.active && !status.prepared &&
        !status.rendering_ready && status.reason, "Initial renderer must be inactive and unready");
    rsf_game_start_args start{sizeof(start), RSF_GAME_ABI_VERSION};
    expect(h.start(&start) == RSF_ERROR_NOT_READY, "Starting before prepare must refuse");
    rsf_game_host_services host{}; host.struct_size = sizeof(host);
    host.abi_version = RSF_GAME_ABI_VERSION; host.session_id = 1;
    rsf_game_prepare_args prepare{sizeof(prepare), RSF_GAME_ABI_VERSION, &host};
    expect(h.prepare(&prepare) == RSF_ERROR_NATIVE_REFUSED,
        "A detection probe must not authorize native hooks in an unrelated process");
    expect(h.status(&status) == RSF_OK && !status.prepared && !status.active,
        "Refused prepare must not leave native state active");
    prepare.abi_version += 1;
    expect(h.prepare(&prepare) == RSF_ERROR_ABI_MISMATCH, "Bad prepare ABI must refuse");
    prepare.abi_version = RSF_GAME_ABI_VERSION; host.struct_size -= 1;
    expect(h.prepare(&prepare) == RSF_ERROR_INVALID_ARGUMENT, "Short host must refuse");
    rsf_game_control_args control{sizeof(control), RSF_GAME_ABI_VERSION};
    expect(h.quiesce(&control) == RSF_OK && h.stop(&control) == RSF_OK,
        "Stopping a refused session must be idempotent");
    std::printf("Anomaly foreign-process admission: %s\n", failures ? "FAILED" : "PASS");
    FreeLibrary(module);
    return failures ? 1 : 0;
}
