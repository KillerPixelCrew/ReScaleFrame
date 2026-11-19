// SPDX-License-Identifier: MIT
#include <rescaleframe/plugin_session.h>
#include <windows.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <filesystem>

namespace {
void check(bool ok, int line) {
    if (!ok) { std::fprintf(stderr, "Plugin session check failed at %d\n", line); std::exit(1); }
}
#define CHECK(x) check((x), __LINE__)
void log(void* user, const char* operation) {
    auto* session = *static_cast<rsf_plugin_session**>(user);
    if (std::strcmp(operation, "start") == 0) {
        rsf_game_renderer_status status{sizeof(status), RSF_GAME_ABI_VERSION};
        CHECK(rsf_plugin_session_status(session, &status) == RSF_ERROR_BUSY);
    } else {
        CHECK(rsf_plugin_session_quiesce(session) == RSF_ERROR_BUSY);
    }
}
}

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const auto path = std::filesystem::absolute(argv[1]);
    const auto name = path.filename();
    rsf_plugin_session* session{};
    rsf_game_probe probe{sizeof(probe)};
    rsf_plugin_session_options options{};
    options.struct_size = sizeof(options);
    options.abi_version = RSF_PLUGIN_SESSION_ABI_VERSION;
    options.plugin_path = path.c_str();
    options.probe = &probe;
    options.services = {sizeof(options.services), RSF_GAME_ABI_VERSION, 1, log, nullptr, &session};
    CHECK(!GetModuleHandleW(name.c_str()));
    alignas(rsf_plugin_session_options) uint32_t short_size = sizeof(uint32_t);
    session = reinterpret_cast<rsf_plugin_session*>(1);
    CHECK(rsf_plugin_session_prepare(reinterpret_cast<const rsf_plugin_session_options*>(&short_size),
                                     &session) == RSF_ERROR_INVALID_ARGUMENT && !session);
    ++options.abi_version;
    CHECK(rsf_plugin_session_prepare(&options, &session) == RSF_ERROR_ABI_MISMATCH && !session);
    --options.abi_version;
    CHECK(rsf_plugin_session_prepare(&options, &session) == RSF_OK && session);
    options.services.log = nullptr; // Plugin must retain the runtime's copy.
    CHECK(rsf_plugin_session_start(session) == RSF_OK);
    CHECK(rsf_plugin_session_start(session) == RSF_OK);
    rsf_game_renderer_status status{sizeof(status), RSF_GAME_ABI_VERSION};
    CHECK(rsf_plugin_session_status(session, &status) == RSF_OK);
    CHECK(rsf_plugin_session_stop(session) == RSF_ERROR_BUSY);
    CHECK(rsf_plugin_session_quiesce(session) == RSF_OK);
    CHECK(rsf_plugin_session_start(session) == RSF_ERROR_NOT_READY);
    CHECK(rsf_plugin_session_stop(session) == RSF_OK && !GetModuleHandleW(name.c_str()));

    options.services.session_id = 2;
    options.services.log = log;
    CHECK(rsf_plugin_session_prepare(&options, &session) == RSF_ERROR_BUSY && session);
    CHECK(GetModuleHandleW(name.c_str()) != nullptr);
    CHECK(rsf_plugin_session_start(session) == RSF_ERROR_NOT_READY);
    CHECK(rsf_plugin_session_quiesce(session) == RSF_OK);
    CHECK(rsf_plugin_session_stop(session) == RSF_ERROR_BUSY);
    CHECK(rsf_plugin_session_status(session, &status) == RSF_OK);
    CHECK(rsf_plugin_session_stop(session) == RSF_OK && !GetModuleHandleW(name.c_str()));
    CHECK(rsf_plugin_session_stop(nullptr) == RSF_OK);
    return 0;
}
