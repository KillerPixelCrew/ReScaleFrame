// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between the native leaves in native_creation.c and the Rust native-creation
// owner (src/creation.rs). Both sides link into ReScaleFrame.Game.SkyrimSE.dll.
// src/platform.rs mirrors the call record; both sides assert its layout.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <wchar.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/d3d11_host.h>

// One guarded creation call, on the detour's stack from enter to leave. The detour fills the
// argument fields and native_result/returned; the Rust owner writes the rest.
typedef struct rsf_skyrim_creation_call {
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

// Detour entry points (src/lib.rs). The Rust session (src/session.rs) registers the platform and
// prepares, starts, quiesces and stops the creation owner. Opens the call's lease and returns the original routine. leave is mandatory after enter, on
// return or unwind; it closes any Runtime scope and restores the native last error.
void* rsf_skyrim_creation_enter(rsf_skyrim_creation_call* call);
void rsf_skyrim_creation_leave(rsf_skyrim_creation_call* call);
