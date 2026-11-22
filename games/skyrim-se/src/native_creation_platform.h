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

#define RSF_SKYRIM_CREATION_PLATFORM_VERSION 3u

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
    // Fault-guarded copy from the process.
    bool (*read)(uintptr_t address, void* out, size_t size);
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
} rsf_skyrim_creation_platform;

// Detour entry points (src/lib.rs). The Rust session (src/session.rs) registers the platform and
// prepares, starts, quiesces and stops the creation owner. Opens the call's lease and returns the original routine. leave is mandatory after enter, on
// return or unwind; it closes any Runtime scope and restores the native last error.
void* rsf_skyrim_creation_enter(rsf_skyrim_creation_call* call);
void rsf_skyrim_creation_leave(rsf_skyrim_creation_call* call);

#ifdef __cplusplus
}
#endif
