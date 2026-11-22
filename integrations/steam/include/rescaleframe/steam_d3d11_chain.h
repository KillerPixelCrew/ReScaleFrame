// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_api.h>
#include <stdbool.h>

// Static integration ABI. Initialize MinHook before calling. The host and cold-owner
// callback are borrowed during this call. The runtime service and its user must outlive
// the installed forwarding. A missing overlay succeeds and is rechecked on the next call.
#ifdef __cplusplus
extern "C" {
#endif
bool rsf_steam_d3d11_chain_prepare(const rsf_game_host_services* host, bool (*cold_owner)(void));
bool rsf_steam_d3d11_chain_prepare_with_user(const rsf_game_host_services* host,
    bool (*cold_owner)(const void*), const void* cold_user);
#ifdef __cplusplus
}
#endif
