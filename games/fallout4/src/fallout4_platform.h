// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 native leaves (host_boundary.cpp) and
// their Rust owner (lib.rs). Not a DLL ABI: both sides link into ReScaleFrame.Game.Fallout4.dll.
// tools/generate-rust-bindings.py generates src/platform_abi.rs from this header.
#include <stdbool.h>
#include <stdint.h>
#include <wchar.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>

#ifdef __cplusplus
extern "C" {
#endif

// The Runtime exports the Rust owner resolves by name.
typedef int32_t (*rsf_fallout4_host_create_fn)(const rsf_d3d11_host_setup*, rsf_game_host_services*);
typedef int32_t (*rsf_fallout4_host_active_fn)(void* services_user, uint32_t active);
typedef int32_t (*rsf_fallout4_creation_begin_fn)(void* services_user, rsf_d3d11_creation_scope*);
typedef int32_t (*rsf_fallout4_creation_end_fn)(void* services_user, const rsf_d3d11_creation_scope*,
    const rsf_d3d11_native_chain*);
typedef int32_t (*rsf_fallout4_bind_presentation_fn)(void* services_user, const rsf_d3d11_presentation_binding*);

// D3D11CreateDeviceAndSwapChain arguments exactly as the guarded creation thunk received them.
// Enumerations travel as their 32-bit values.
typedef struct rsf_fallout4_create_call {
    void* adapter;
    int32_t driver_type;
    void* software;
    uint32_t flags;
    const int32_t* feature_levels;
    uint32_t feature_level_count;
    uint32_t sdk_version;
    const void* swapchain_desc;
    void** swapchain;
    void** device;
    int32_t* feature_level;
    void** immediate_context;
} rsf_fallout4_create_call;

// Linked native exception guards. False signals a native exception.
#ifdef __cplusplus
bool rsf_fallout4_native_log(rsf_game_log_fn log, void* user, const char* message) noexcept;
bool rsf_fallout4_native_host_create(rsf_fallout4_host_create_fn create, const rsf_d3d11_host_setup* setup,
    rsf_game_host_services* services, int32_t* result) noexcept;
bool rsf_fallout4_native_host_set_active(rsf_fallout4_host_active_fn activate, void* user, uint32_t active,
    int32_t* result) noexcept;
bool rsf_fallout4_native_creation_begin(rsf_fallout4_creation_begin_fn begin, void* user,
    rsf_d3d11_creation_scope* scope, int32_t* result) noexcept;
bool rsf_fallout4_native_creation_end(rsf_fallout4_creation_end_fn end, void* user,
    const rsf_d3d11_creation_scope* scope, const rsf_d3d11_native_chain* outputs, int32_t* result) noexcept;
bool rsf_fallout4_native_bind_presentation(rsf_fallout4_bind_presentation_fn bind, void* user,
    const rsf_d3d11_presentation_binding* binding, int32_t* result) noexcept;
int32_t rsf_fallout4_call_creation(void* original, const rsf_fallout4_create_call* call) noexcept;
#endif

#ifdef __cplusplus
}
#endif
