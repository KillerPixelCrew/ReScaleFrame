// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Steam chain facade (d3d11_chain.cpp) and its Rust owner (lib.rs).
// Not a DLL ABI: both sides are linked into the same adapter. tools/generate-rust-bindings.py
// generates chain_abi.rs from this header.
#include <stdbool.h>
#include <stdint.h>
#include <wchar.h>
#include <rescaleframe/d3d11_host.h>

#ifdef __cplusplus
extern "C" {
#endif

// The Runtime's rsf_d3d11_host_dispatch_chain export.
typedef int32_t (*rsf_steam_chain_service_fn)(void* services_user, rsf_d3d11_chain_dispatch* request);

// Native operations the Rust owner cannot express without a C++ or MinHook dependency. The table
// lives for the process: installed relays and hooks read it after prepare returns.
typedef struct rsf_steam_chain_platform {
    // MinHook on the adapter's process instance; the adapter initializes it before prepare.
    bool (*create_hook)(void* target, void* detour, void** original);
    bool (*queue_hook)(void* target, bool enable);
    bool (*apply_queued)(void);
    // Calls the Runtime chain service. A thrown exception reports RSF_ERROR_NOT_READY.
    int32_t (*dispatch)(rsf_steam_chain_service_fn service, void* services_user, rsf_d3d11_chain_dispatch* request);
    // Logs through the borrowed host services. A thrown exception is swallowed.
    void (*log)(const rsf_game_host_services* host, const char* message);
    // Lower-case hex SHA-256 of a file into 64 characters, without a terminator.
    bool (*file_sha256)(const wchar_t* path, char* digest);
} rsf_steam_chain_platform;

// Installs Steam's D3D11 swapchain forwarding once per process. A missing overlay module is
// success and is rechecked on the next call. host and cold_user are borrowed for this call;
// platform must live for the process.
bool rsf_steam_d3d11_chain_prepare(const rsf_game_host_services* host, bool (*cold_owner)(const void*),
    const void* cold_user, const rsf_steam_chain_platform* platform);

#ifdef __cplusplus
}
#endif
