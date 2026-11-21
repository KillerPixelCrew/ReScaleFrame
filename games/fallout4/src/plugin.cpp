// SPDX-License-Identifier: MIT
// GameSDK export and native leaves of the Rust Fallout 4 owner (lib.rs). Rust owns the exact
// build identity, the lifecycle and its Runtime session, DPI startup, the cold creation
// sequence and the CPU event owner. This file supplies MinHook, exception guards, the shared file
// hash, the shared Steam chain facade and the C++ render subsystem; cpu_events.cpp supplies the
// CPU leaves.
#include "fallout4_platform.h"
#include "native_creation.h"
#include "cpu_events.h"
#include "render_hooks.h"
#include "render_inputs.h"
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/steam_d3d11_chain.h>
#include <MinHook.h>
#include <cstring>
#include <string>

namespace {
using namespace rsf::fallout4;
static_assert(sizeof(rsf_fallout4_platform) == 14 * sizeof(void*));

// A native call that throws reports false; the Rust lifecycle then answers RSF_ERROR_NOT_READY
// as the previous native catch did.
template<class Call> bool guarded(Call&& call) noexcept {
    try { call(); return true; } catch (...) { return false; }
}

bool subsystem(uint32_t operation, const rsf_game_host_services* services, bool* value) noexcept {
    return guarded([&] {
        switch (operation) {
        case RSF_FALLOUT4_VERIFY_RENDER: *value = verify_render_hooks() && verify_render_inputs(); break;
        case RSF_FALLOUT4_PREPARE_RENDER: *value = services && prepare_render_hooks(*services); break;
        case RSF_FALLOUT4_START_RENDER: *value = start_render_hooks(); break;
        case RSF_FALLOUT4_QUIESCE_RENDER: quiesce_render_hooks(); *value = true; break;
        case RSF_FALLOUT4_STOP_RENDER: *value = stop_render_hooks(); break;
        case RSF_FALLOUT4_RENDER_BUSY: *value = render_hooks_busy(); break;
        default: *value = false; break;
        }
    });
}

constexpr rsf_fallout4_platform platform{
    [](rsf_game_log_fn log, void* user, const char* message) noexcept {
        return guarded([&] { if (log) log(user, message); });
    },
    [](const wchar_t* path, char* digest, bool* hashed) noexcept {
        return guarded([&] {
            std::string hash;
            *hashed = rsf::sha256_file(path, hash) && hash.size() == 64;
            if (*hashed) std::memcpy(digest, hash.data(), hash.size());
        });
    },
    [](const rsf_game_host_services* host) noexcept {
        return rsf::steam::prepare_d3d11_chain(*host, [] { return rsf_fallout4_cold_owner(); });
    },
    []() noexcept {
        const auto result = MH_Initialize();
        return result == MH_OK || result == MH_ERROR_ALREADY_INITIALIZED;
    },
    [](void* target) noexcept { return create_creation_hook(target); },
    [](void* target) noexcept { return MH_EnableHook(target) == MH_OK; },
    [](void* target) noexcept {
        const auto result = MH_DisableHook(target);
        return result == MH_OK || result == MH_ERROR_DISABLED;
    },
    [](const rsf_fallout4_create_call* call) noexcept { return call_creation(call); },
    [](rsf_fallout4_host_create_fn create, const rsf_d3d11_host_setup* setup, rsf_game_host_services* services,
        int32_t* result) noexcept {
        return guarded([&] { *result = create ? create(setup, services) : RSF_ERROR_NOT_READY; });
    },
    [](rsf_fallout4_host_active_fn activate, void* user, uint32_t active, int32_t* result) noexcept {
        return guarded([&] { *result = activate ? activate(user, active) : RSF_ERROR_NOT_READY; });
    },
    [](rsf_fallout4_creation_begin_fn begin, void* user, rsf_d3d11_creation_scope* scope, int32_t* result) noexcept {
        return guarded([&] { *result = begin ? begin(user, scope) : RSF_ERROR_NOT_READY; });
    },
    [](rsf_fallout4_creation_end_fn end, void* user, const rsf_d3d11_creation_scope* scope,
        const rsf_d3d11_native_chain* outputs, int32_t* result) noexcept {
        return guarded([&] { *result = end ? end(user, scope, outputs) : RSF_ERROR_NOT_READY; });
    },
    [](rsf_fallout4_bind_presentation_fn bind, void* user, const rsf_d3d11_presentation_binding* binding,
        int32_t* result) noexcept {
        return guarded([&] { *result = bind ? bind(user, binding) : RSF_ERROR_NOT_READY; });
    },
    subsystem,
};
}
extern "C" __declspec(dllexport) rsf_result rsf_get_game_plugin_api(uint32_t requested_abi, rsf_game_plugin_api* api) noexcept {
    // The CPU leaves are constant and complete; prepare refuses CPU hooks if they are unbound.
    (void)rsf_fallout4_cpu_publish(rsf::fallout4::cpu_event_platform());
    return rsf_fallout4_publish_api(requested_abi, api, &platform);
}
