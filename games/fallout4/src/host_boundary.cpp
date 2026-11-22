// SPDX-License-Identifier: MIT
// C++ exception guards and the Steam and render bridges used by the Rust GameSDK export.
#include "fallout4_platform.h"
#include "render_hooks_platform.h"
#include <rescaleframe/sha256_file.h>
#include <rescaleframe/steam_d3d11_chain.h>
#include <cstring>
#include <string>

namespace {
using namespace rsf::fallout4;
static_assert(sizeof(rsf_fallout4_platform) == 9 * sizeof(void*));

// A native call that throws reports false; the Rust lifecycle then answers RSF_ERROR_NOT_READY
// as the previous native catch did.
template<class Call> bool guarded(Call&& call) noexcept {
    try { call(); return true; } catch (...) { return false; }
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
    rsf_fallout4_call_creation,
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
};
}
extern "C" const rsf_fallout4_platform* rsf_fallout4_host_boundary() noexcept {
    return &platform;
}

extern "C" void rsf_fallout4_publish_render_boundary() noexcept {
    (void)rsf::fallout4::rsf_fallout4_render_publish(rsf::fallout4::render_hook_leaves());
}
