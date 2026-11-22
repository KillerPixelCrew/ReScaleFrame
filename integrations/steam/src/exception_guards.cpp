// SPDX-License-Identifier: MIT
#include <rescaleframe/d3d11_host.h>
#include <cstddef>

static_assert(sizeof(rsf_d3d11_chain_dispatch) == 72 && offsetof(rsf_d3d11_chain_dispatch, args) == 24 &&
    offsetof(rsf_d3d11_chain_dispatch, result) == 64);

extern "C" int32_t rsf_steam_dispatch_guard(decltype(&rsf_d3d11_host_dispatch_chain) service,
    void* user, rsf_d3d11_chain_dispatch* request) noexcept
{
    try { return service(user, request); }
    catch (...) { return RSF_ERROR_NOT_READY; }
}

extern "C" void rsf_steam_log_guard(const rsf_game_host_services* host, const char* message) noexcept
{
    if (host->log) try { host->log(host->user, message); } catch (...) {}
}
