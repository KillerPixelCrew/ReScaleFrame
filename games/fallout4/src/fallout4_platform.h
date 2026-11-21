// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the Fallout 4 native leaves (plugin.cpp, native_creation.cpp) and
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

// Native render and CPU subsystems that remain C++ (render_hooks, render_inputs, cpu_events).
#define RSF_FALLOUT4_VERIFY_RENDER 0u  /* verify_render_hooks() && verify_render_inputs() */
#define RSF_FALLOUT4_PREPARE_RENDER 1u /* prepare_render_hooks(services) */
#define RSF_FALLOUT4_PREPARE_CPU 2u    /* prepare_cpu_events(services) */
#define RSF_FALLOUT4_START_RENDER 3u
#define RSF_FALLOUT4_START_CPU 4u
#define RSF_FALLOUT4_QUIESCE_RENDER 5u /* value is always true */
#define RSF_FALLOUT4_QUIESCE_CPU 6u    /* value is always true */
#define RSF_FALLOUT4_STOP_RENDER 7u
#define RSF_FALLOUT4_STOP_CPU 8u
#define RSF_FALLOUT4_RENDER_BUSY 9u
#define RSF_FALLOUT4_CPU_BUSY 10u

// Leaves the Rust owner cannot express without C++, MinHook or a typed native call. A guarded
// leaf returns false only when its native call threw; its result goes to the out parameter.
// The table lives for the process: the installed creation hook reads it after prepare returns.
typedef struct rsf_fallout4_platform {
    // Guarded host log. A null log completes without a call.
    bool (*log)(rsf_game_log_fn log, void* user, const char* message);
    // Guarded lower-case hex SHA-256 of a file into 64 characters, without a terminator.
    bool (*file_sha256)(const wchar_t* path, char* digest, bool* hashed);
    // The shared Steam D3D11 chain facade, given this adapter's cold-owner check.
    bool (*prepare_steam_chain)(const rsf_game_host_services* host);
    // MinHook on the process instance. Initialize accepts an existing instance; disable accepts
    // an already disabled hook. The creation hook keeps its trampoline natively.
    bool (*initialize_hooks)(void);
    bool (*create_creation_hook)(void* target);
    bool (*enable_hook)(void* target);
    bool (*disable_hook)(void* target);
    // The original creation through its trampoline, with the thread's last error unchanged.
    int32_t (*call_create)(const rsf_fallout4_create_call* call);
    // Guarded Runtime calls.
    bool (*host_create)(rsf_fallout4_host_create_fn create, const rsf_d3d11_host_setup* setup,
        rsf_game_host_services* services, int32_t* result);
    bool (*host_set_active)(rsf_fallout4_host_active_fn activate, void* services_user, uint32_t active,
        int32_t* result);
    bool (*creation_begin)(rsf_fallout4_creation_begin_fn begin, void* services_user,
        rsf_d3d11_creation_scope* scope, int32_t* result);
    bool (*creation_end)(rsf_fallout4_creation_end_fn end, void* services_user,
        const rsf_d3d11_creation_scope* scope, const rsf_d3d11_native_chain* outputs, int32_t* result);
    bool (*bind_presentation)(rsf_fallout4_bind_presentation_fn bind, void* services_user,
        const rsf_d3d11_presentation_binding* binding, int32_t* result);
    // Guarded subsystem operation (RSF_FALLOUT4_*). services is read only by the prepare operations.
    bool (*subsystem)(uint32_t operation, const rsf_game_host_services* services, bool* value);
} rsf_fallout4_platform;

// Rust owner entry points.
// Publishes the GameSDK table. platform must be complete and live for the process.
rsf_result rsf_fallout4_publish_api(uint32_t requested_abi, rsf_game_plugin_api* api,
    const rsf_fallout4_platform* platform);
// The creation thunk detour body. caller is the detour's return address.
int32_t rsf_fallout4_creation_observe(uintptr_t caller, const rsf_fallout4_create_call* call);
// Exact executable name and SHA-256, for the C++ subsystem guards.
bool rsf_fallout4_verify_executable(void);
// The native renderer owner's window in this process.
int rsf_fallout4_accept_window(void* user, void* window);
// The renderer owner has not created its device yet.
bool rsf_fallout4_cold_owner(void);

#ifdef __cplusplus
}
#endif
