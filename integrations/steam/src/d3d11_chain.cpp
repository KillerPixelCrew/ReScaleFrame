// SPDX-License-Identifier: MIT
// Native facade for the Rust Steam chain owner (lib.rs). It keeps the adapters' C++ entry point
// and supplies only MinHook, exception guards and the shared file hash.
#include <rescaleframe/steam_d3d11_chain.h>
#include "d3d11_chain_platform.h"
#include <rescaleframe/sha256_file.h>
#include <MinHook.h>
#include <windows.h>
#include <cstddef>
#include <cstring>
#include <string>

namespace rsf::steam {
namespace {
static_assert(sizeof(rsf_steam_chain_platform) == 6 * sizeof(void*));
static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72 && offsetof(rsf_d3d11_chain_dispatch, args) == 24 &&
    offsetof(rsf_d3d11_chain_dispatch, result) == 64);

constexpr rsf_steam_chain_platform platform{
    [](void* target, void* detour, void** original) noexcept {
        return MH_CreateHook(target, detour, original) == MH_OK;
    },
    [](void* target, bool enable) noexcept {
        return (enable ? MH_QueueEnableHook(target) : MH_QueueDisableHook(target)) == MH_OK;
    },
    []() noexcept { return MH_ApplyQueued() == MH_OK; },
    [](rsf_steam_chain_service_fn service, void* user, rsf_d3d11_chain_dispatch* request) noexcept {
        int32_t status = RSF_ERROR_NOT_READY;
        try { status = service(user, request); } catch (...) {}
        return status;
    },
    [](const rsf_game_host_services* host, const char* message) noexcept {
        if (host->log) try { host->log(host->user, message); } catch (...) {}
    },
    [](const wchar_t* path, char* digest) noexcept {
        try {
            std::string hash;
            if (!rsf::sha256_file(path, hash) || hash.size() != 64) return false;
            std::memcpy(digest, hash.data(), hash.size());
            return true;
        } catch (...) { return false; }
    },
};
}
bool prepare_d3d11_chain(const rsf_game_host_services& host, bool (*cold_owner)()) noexcept {
    using ColdOwner = bool (*)();
    const auto cold = [](const void* user) noexcept {
        try { return (*static_cast<const ColdOwner*>(user))(); }
        catch (...) { return false; }
    };
    return rsf_steam_d3d11_chain_prepare(&host, cold_owner ? +cold : nullptr, &cold_owner, &platform);
}
}
