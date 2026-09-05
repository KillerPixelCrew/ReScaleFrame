#include <rescaleframe/game_api.h>
#include <windows.h>

#include <iostream>

namespace {
bool check(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << message << '\n';
    }
    return condition;
}
}

int wmain(int argc, wchar_t* argv[])
{
    if (argc != 2) {
        return 2;
    }
    const HMODULE module = LoadLibraryExW(argv[1], nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!check(module != nullptr, "Plugin DLL could not be loaded.")) {
        return 1;
    }
    const auto entry = reinterpret_cast<rsf_get_game_plugin_api_fn>(
        GetProcAddress(module, RSF_GAME_ENTRY_POINT));
    if (!check(entry != nullptr, "Plugin entry point is missing.")) {
        FreeLibrary(module);
        return 1;
    }

    bool passed = check(entry(RSF_GAME_ABI_VERSION, nullptr) == RSF_ERROR_INVALID_ARGUMENT,
                        "A null output must be rejected.");
    rsf_game_plugin_api api{};
    passed &= check(entry(RSF_GAME_ABI_VERSION, &api) == RSF_ERROR_INVALID_ARGUMENT,
                    "A short output structure must be rejected.");
    api.struct_size = sizeof(api);
    passed &= check(entry(RSF_GAME_ABI_VERSION + 1, &api) == RSF_ERROR_ABI_MISMATCH,
                    "An incompatible ABI must be rejected.");
    passed &= check(entry(RSF_GAME_ABI_VERSION, &api) == RSF_OK,
                    "The current ABI must be accepted.");
    if (!passed || !check(api.detect != nullptr, "Detection callback is missing.")) {
        FreeLibrary(module);
        return 1;
    }

    rsf_game_probe probe{sizeof(rsf_game_probe), 0x8664, 0, "Ace7Game.exe",
        "c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f"};
    passed &= check(api.detect(&probe) == RSF_GAME_RECOGNIZED,
                    "The researched executable must be recognized.");
    passed &= check(api.info.rendering_ready == 0,
                    "Recognition must not advertise unimplemented rendering support.");
    probe.sha256_hex = "unknown";
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN,
                    "An unrecognized hash must not match by name alone.");
    probe.sha256_hex = nullptr;
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN,
                    "A missing fingerprint must not match.");
    passed &= check(api.detect(nullptr) == RSF_GAME_UNKNOWN,
                    "A null probe must not match.");
    FreeLibrary(module);
    return passed ? 0 : 1;
}
