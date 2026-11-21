// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the native leaves in native_creation.cpp and the Rust native-creation
// owner (src/creation.rs, crate rsf-skyrim-se). Not a DLL ABI: both sides link into
// ReScaleFrame.Game.SkyrimSE.dll. src/platform.rs mirrors these layouts by hand (bindgen-cli 0.72.1
// was not available) and both sides assert sizes and offsets. No Skyrim address, expected byte,
// route profile or admission decision lives in the leaves.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <wchar.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_SKYRIM_CREATION_PLATFORM_VERSION 1u

// VirtualQuery facts for one address.
typedef struct rsf_skyrim_memory_region {
    uintptr_t base;
    uintptr_t allocation_base;
    uintptr_t size;
    uint32_t state;
    uint32_t protect;
} rsf_skyrim_memory_region;

// One guarded creation call, on the detour's stack from enter to leave. The detour fills the
// argument fields and native_result/returned; the Rust owner writes the rest.
typedef struct rsf_skyrim_creation_call {
    uint32_t struct_size, version;
    uintptr_t caller;        // native return address of the call into the game thunk
    const void* description; // DXGI_SWAP_CHAIN_DESC*
    void** swapchain;        // IDXGISwapChain** output
    void** device;           // ID3D11Device** output
    void** context;          // ID3D11DeviceContext** output
    int32_t native_result;   // E_FAIL until the original call returns
    uint8_t returned;
    uint8_t begun, matched, reserved;
    uintptr_t owner;
    void* window;
    void* outer;
    rsf_d3d11_creation_scope scope;
    rsf_d3d11_native_chain outputs;
} rsf_skyrim_creation_call;

typedef struct rsf_skyrim_creation_platform {
    uint32_t struct_size, version;
    uint32_t d3d11_host_abi; // RSF_D3D11_HOST_ABI_VERSION these leaves were built against
    uint32_t reserved;
    // Process image, modules and memory. read is a fault-guarded copy.
    uintptr_t (*main_module)(void);
    uintptr_t (*module_handle)(const wchar_t* name);
    uintptr_t (*export_address)(uintptr_t module, const char* name);
    bool (*read)(uintptr_t address, void* out, size_t size);
    bool (*query)(uintptr_t address, rsf_skyrim_memory_region* out);
    // Lower-case hex SHA-256 of the module's file into 64 characters, without a terminator.
    bool (*module_sha256)(uintptr_t module, char* digest);
    // GetModuleHandleExW(FROM_ADDRESS | PIN); returns the pinned module or 0.
    uintptr_t (*pin_module)(uintptr_t address);
    uint32_t (*last_error)(void);
    void (*set_last_error)(uint32_t error);
    // MinHook on the adapter's process instance. Enable also accepts an already enabled hook and
    // disable an already disabled one.
    bool (*hook_create)(uintptr_t target, const void* detour, void** original);
    bool (*hook_enable)(uintptr_t target);
    bool (*hook_disable)(uintptr_t target);
    // D3D11CreateDeviceAndSwapChain-shaped detour that captures the native return address.
    const void* create_detour;
    // Logs through borrowed host services. A thrown exception is swallowed.
    void (*log)(const rsf_game_host_services* host, const char* message);
    // Call the Runtime's rsf_d3d11_host_creation_begin/end at service. A thrown exception
    // reports RSF_ERROR_NOT_READY.
    int32_t (*creation_begin)(uintptr_t service, void* user, rsf_d3d11_creation_scope* scope);
    int32_t (*creation_end)(uintptr_t service, void* user, const rsf_d3d11_creation_scope* scope,
        const rsf_d3d11_native_chain* outputs);
    // The Community Shaders compatibility gate (compatibility.cpp).
    bool (*render_allowed)(void);
    // The shared Steam overlay facade, rsf::steam::prepare_d3d11_chain.
    bool (*steam_prepare)(const rsf_game_host_services* host, bool (*cold_owner)(void));
} rsf_skyrim_creation_platform;

// Rust owner entry points (src/lib.rs). platform must live for the process; host is borrowed.
bool rsf_skyrim_creation_prepare(const rsf_game_host_services* host,
    const rsf_skyrim_creation_platform* platform);
bool rsf_skyrim_creation_start(void);
void rsf_skyrim_creation_quiesce(void);
bool rsf_skyrim_creation_stop(void);
bool rsf_skyrim_creation_busy(void);
// Opens the call's lease and returns the original routine. leave is mandatory after enter, on
// return or unwind; it closes any Runtime scope and restores the native last error.
void* rsf_skyrim_creation_enter(rsf_skyrim_creation_call* call);
void rsf_skyrim_creation_leave(rsf_skyrim_creation_call* call);

#ifdef __cplusplus
}
#endif
