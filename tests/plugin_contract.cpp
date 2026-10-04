#include <rescaleframe/game_api.h>
#include <rescaleframe/game_frame.h>
#include <windows.h>

#include <cwchar>
#include <cstring>
#include <iostream>

namespace {
struct Fixture {
    const wchar_t* argument;
    const char* id;
    const char* executable;
    const char* sha256;
    rsf_result prepare_result;
};

constexpr Fixture fixtures[] = {
    {L"ac7", "ac7", "Ace7Game.exe",
     "c7da97f5f8a807d4f1264adbb074146fcffe9bdc2ffa98791b822cd28e558f4f",
     RSF_ERROR_NATIVE_REFUSED},
    {L"project-wingman", "project-wingman", "ProjectWingman-Win64-Shipping.exe",
     "092e84225624a4de9c05d2404ff55269bd4a2aa2ff0548f2a183c6bf36abc85a",
     RSF_ERROR_NOT_READY},
    {L"unity-mono", "unity-mono", "DragNWash.exe",
     "5fdfffe386a2f43b77626cd3d70554d84c6588c94d309544924d6fab088ddafc",
     RSF_ERROR_NOT_READY}};

bool check(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << message << '\n';
    }
    return condition;
}
}

// The LOAD_LIBRARY_SEARCH_* flags only accept a fully qualified path, so resolve the plugin
// against this executable's own directory. That is where it sits in the build tree and next to
// the host in a real load, and it keeps the path valid under a cross-build emulator, which
// cannot be handed a host path.
bool resolve_beside_self(const wchar_t* leaf, wchar_t* buffer, DWORD capacity)
{
    const DWORD length = GetModuleFileNameW(nullptr, buffer, capacity);
    if (length == 0 || length >= capacity) {
        return false;
    }
    wchar_t* const separator = wcsrchr(buffer, L'\\');
    if (!separator) {
        return false;
    }
    separator[1] = L'\0';
    return wcslen(buffer) + wcslen(leaf) < capacity && wcscat(buffer, leaf) != nullptr;
}

int wmain(int argc, wchar_t* argv[])
{
    if (argc != 3) {
        return 2;
    }
    const Fixture* fixture = nullptr;
    for (const auto& candidate : fixtures) {
        if (wcscmp(argv[2], candidate.argument) == 0) {
            fixture = &candidate;
            break;
        }
    }
    if (!fixture) {
        return 2;
    }
    wchar_t path[MAX_PATH];
    if (!check(resolve_beside_self(argv[1], path, MAX_PATH), "Plugin path could not be resolved.")) {
        return 1;
    }
    const HMODULE module = LoadLibraryExW(path, nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
    if (!check(module != nullptr, "Plugin DLL could not be loaded.")) {
        return 1;
    }
    // Going through void* is the portable spelling of a GetProcAddress cast. Casting FARPROC
    // straight to the target signature is a function-type mismatch that GCC rejects.
    const auto entry = reinterpret_cast<rsf_get_game_plugin_api_fn>(
        reinterpret_cast<void*>(GetProcAddress(module, RSF_GAME_ENTRY_POINT)));
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

    passed &= check(api.info.id && std::strcmp(api.info.id, fixture->id) == 0,
                    "The plugin must report the expected game identity.");
    passed &= check(api.info.name && api.info.version && api.info.status,
                    "The plugin must supply its immutable metadata strings.");
    rsf_game_probe probe{sizeof(rsf_game_probe), 0x8664, 0, fixture->executable, fixture->sha256};
    passed &= check(api.detect(&probe) == RSF_GAME_RECOGNIZED,
                    "The researched executable must be recognized.");
    passed &= check(api.info.rendering_ready == 0,
                    "Recognition must not advertise unimplemented rendering support.");
    if (!check(api.hooks.prepare && api.hooks.start && api.hooks.quiesce && api.hooks.stop && api.hooks.status,
               "The game must expose its renderer lifecycle.")) {
        FreeLibrary(module);
        return 1;
    }
    rsf_game_renderer_status status{};
    passed &= check(api.hooks.status(&status) == RSF_ERROR_INVALID_ARGUMENT, "Short renderer status must refuse.");
    status.struct_size = sizeof(status); status.abi_version = RSF_GAME_ABI_VERSION;
    passed &= check(api.hooks.status(&status) == RSF_OK && !status.prepared && !status.active && !status.rendering_ready,
                    "The renderer must start inactive and unvalidated.");
    rsf_game_start_args start{sizeof(start), RSF_GAME_ABI_VERSION};
    passed &= check(api.hooks.start(&start) == RSF_ERROR_NOT_READY, "Activation requires native preparation.");
    rsf_game_host_services host{sizeof(host), RSF_GAME_ABI_VERSION, 9, nullptr, nullptr, nullptr};
    rsf_game_prepare_args prepare{sizeof(prepare), RSF_GAME_ABI_VERSION, &host};
    passed &= check(api.hooks.prepare(&prepare) == fixture->prepare_result,
                    "Preparation must refuse without implemented, validated native hooks.");
    rsf_game_control_args control{sizeof(control), RSF_GAME_ABI_VERSION};
    passed &= check(api.hooks.quiesce(&control) == RSF_OK && api.hooks.stop(&control) == RSF_OK,
                    "Refused preparation must be safely stoppable.");
    passed &= check(api.hooks.status(&status) == RSF_OK && !status.prepared && !status.active && !status.rendering_ready,
                    "Refused preparation and cleanup must leave no active renderer.");
    probe.pe_machine = 0x14c;
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN, "A different architecture must not match.");
    probe.pe_machine = 0x8664;
    probe.struct_size = sizeof(probe) - 1;
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN, "A short probe must not match.");
    probe.struct_size = sizeof(probe);
    probe.executable_name_utf8 = "OtherGame.exe";
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN, "A fingerprint with a different name must not match.");
    probe.executable_name_utf8 = nullptr;
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN, "A missing executable name must not match.");
    probe.executable_name_utf8 = fixture->executable;

    if (fixture->prepare_result == RSF_ERROR_NOT_READY) {
        passed &= check(api.hooks.prepare(nullptr) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.start(nullptr) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.quiesce(nullptr) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.stop(nullptr) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.status(nullptr) == RSF_ERROR_INVALID_ARGUMENT,
                        "Scaffold lifecycle callbacks must reject null structures.");
        prepare.struct_size = sizeof(prepare) - 1;
        start.struct_size = sizeof(start) - 1;
        control.struct_size = sizeof(control) - 1;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.start(&start) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.quiesce(&control) == RSF_ERROR_INVALID_ARGUMENT &&
                        api.hooks.stop(&control) == RSF_ERROR_INVALID_ARGUMENT,
                        "Scaffold lifecycle callbacks must reject short structures.");
        prepare.struct_size = sizeof(prepare);
        start.struct_size = sizeof(start);
        control.struct_size = sizeof(control);
        prepare.abi_version = start.abi_version = control.abi_version = status.abi_version = RSF_GAME_ABI_VERSION + 1;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_ABI_MISMATCH &&
                        api.hooks.start(&start) == RSF_ERROR_ABI_MISMATCH &&
                        api.hooks.quiesce(&control) == RSF_ERROR_ABI_MISMATCH &&
                        api.hooks.stop(&control) == RSF_ERROR_ABI_MISMATCH &&
                        api.hooks.status(&status) == RSF_ERROR_ABI_MISMATCH,
                        "Scaffold lifecycle callbacks must reject incompatible ABIs.");
        prepare.abi_version = start.abi_version = control.abi_version = status.abi_version = RSF_GAME_ABI_VERSION;
        prepare.host = nullptr;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_INVALID_ARGUMENT, "A missing host must refuse.");
        prepare.host = &host;
        host.struct_size = sizeof(host) - 1;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_INVALID_ARGUMENT, "A short host must refuse.");
        host.struct_size = sizeof(host);
        host.abi_version = RSF_GAME_ABI_VERSION + 1;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_ABI_MISMATCH, "An incompatible host must refuse.");
        host.abi_version = RSF_GAME_ABI_VERSION;
        host.session_id = 0;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_INVALID_ARGUMENT, "A missing session must refuse.");
        host.session_id = 9;
        passed &= check(api.hooks.prepare(&prepare) == RSF_ERROR_NOT_READY &&
                        api.hooks.start(&start) == RSF_ERROR_NOT_READY &&
                        api.hooks.quiesce(&control) == RSF_OK && api.hooks.stop(&control) == RSF_OK,
                        "The scaffold must remain inactive and allow repeated cleanup.");
        status.prepared = status.active = status.rendering_ready = 1;
        passed &= check(api.hooks.status(&status) == RSF_OK && !status.prepared && !status.active &&
                        !status.rendering_ready && status.reason,
                        "Status must overwrite stale flags and explain missing renderer support.");
    }
    probe.sha256_hex = "unknown";
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN,
                    "An unrecognized hash must not match by name alone.");
    probe.sha256_hex = nullptr;
    passed &= check(api.detect(&probe) == RSF_GAME_UNKNOWN,
                    "A missing fingerprint must not match.");
    passed &= check(api.detect(nullptr) == RSF_GAME_UNKNOWN,
                    "A null probe must not match.");

    // The frame record's eligibility rules. They are asked in several places and the answer has to
    // be the same in all of them, so they live in the header as inline functions and are pinned
    // here rather than being restated by each caller.
    {
        rsf_frame_record record{};
        record.struct_size = sizeof(record);
        record.abi_version = RSF_GAME_FRAME_ABI_VERSION;

        record.screen = RSF_SCREEN_FLIGHT;
        passed &= check(rsf_frame_allows_sr(&record) && rsf_frame_allows_fg(&record),
                        "Flight must allow both, which is the case the project exists for.");

        record.screen = RSF_SCREEN_VIDEO;
        passed &= check(!rsf_frame_allows_sr(&record) && !rsf_frame_allows_fg(&record),
                        "A video must allow neither: reconstructing one softens it and "
                        "interpolating one smears a cut.");

        record.screen = RSF_SCREEN_LOADING;
        passed &= check(!rsf_frame_allows_sr(&record), "Nor a loading screen.");

        record.screen = RSF_SCREEN_MENU;
        passed &= check(rsf_frame_allows_sr(&record) && !rsf_frame_allows_fg(&record),
                        "A menu may be reconstructed but never interpolated: it holds still and "
                        "then jumps, which is no motion to work from followed by a discontinuity "
                        "to smear.");

        record.screen = RSF_SCREEN_UNKNOWN;
        passed &= check(!rsf_frame_allows_fg(&record),
                        "And unknown is read conservatively rather than as flight, or a policy "
                        "that has not recognised a cutscene will interpolate one.");

        record.screen = RSF_SCREEN_FLIGHT;
        record.flags = RSF_FRAME_FLAG_NO_FG;
        passed &= check(rsf_frame_allows_sr(&record) && !rsf_frame_allows_fg(&record),
                        "A per-frame refusal must be independent of the screen.");
        record.flags = RSF_FRAME_FLAG_NO_SR;
        passed &= check(!rsf_frame_allows_sr(&record) && !rsf_frame_allows_fg(&record),
                        "And refusing reconstruction must refuse generation with it, since "
                        "generation feeds on what reconstruction produced.");
        record.flags = 0;

        record.struct_size = 8;
        passed &= check(!rsf_frame_allows_sr(&record) && !rsf_frame_allows_fg(&record),
                        "A short record must refuse rather than read fields that may not be "
                        "there: this header is compiled by plugins built against older versions.");
    }

    FreeLibrary(module);
    return passed ? 0 : 1;
}
