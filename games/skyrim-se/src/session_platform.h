// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between plugin.cpp and the Rust session owner (src/session.rs, crate
// rsf-skyrim-se). Not a DLL ABI: both sides link into ReScaleFrame.Game.SkyrimSE.dll.
// src/session.rs mirrors the private leaves. The public setup comes from the generated SDK binding.
// Rust owns the lifecycle and the borrowed services. The leaves expose the native
// hook entry points and contain foreign host and Runtime exceptions.
#include "observer_platform.h"

#define RSF_SKYRIM_SESSION_LEAVES_VERSION 4u

// The leaves are synchronous and live for the process. Runtime and host leaves
// report native exceptions separately from the callback result.
struct rsf_skyrim_session_leaves {
    uint32_t struct_size, version;
    const rsf_skyrim_creation_platform* platform;
    const rsf_skyrim_observer_leaves* observers;
    int32_t (*admit_observers)();
    bool (*observers_busy)();
    bool (*cpu_prepare)();
    bool (*cpu_start)();
    // compatibility_checkpoint; arm also installs rsf_skyrim_session_withdraw. Never throws.
    bool (*checkpoint)(const char** refusal, bool arm);
    const char* (*compatibility_status)();
    // Guarded host log. A null log completes without a call.
    bool (*log)(const rsf_game_host_services* host, const char* message);
    // Guarded Runtime rsf_d3d11_host_create and rsf_d3d11_host_set_active at the given exports.
    bool (*host_create)(uintptr_t create, const rsf_d3d11_host_setup* setup,
        rsf_game_host_services* services, int32_t* result);
    bool (*host_set_active)(uintptr_t activate, void* user, uint32_t active, int32_t* result);
    // The game window class policy lent to the presentation bridge.
    int (*accept_window)(void* user, void* window);
};
static_assert(sizeof(rsf_skyrim_session_leaves) == 8 + 13 * sizeof(void*));
static_assert(sizeof(rsf_motion_decode_params) == 48);
static_assert(offsetof(rsf_d3d11_host_setup, depth_far_plane) == 160 && offsetof(rsf_d3d11_host_setup, motion) == 80 &&
    offsetof(rsf_d3d11_host_setup, quality_choices) == 128 &&
    offsetof(rsf_d3d11_host_setup, accept_window) == 136 &&
    offsetof(rsf_d3d11_host_setup, colour_transport_default) == 156);

// Rust session owner. The detours read the borrowed services only through the lease, under the
// same session lock as every lifecycle call and the compatibility withdraw callback.
extern "C" rsf_result rsf_skyrim_publish_api(uint32_t requested, rsf_game_plugin_api* out,
    const rsf_skyrim_session_leaves* leaves) noexcept;
extern "C" bool rsf_skyrim_session_accepting() noexcept;
// Copies the services into out: while accepting when live, otherwise as retained. Returns the
// Runtime rsf_d3d11_host_bind_presentation export, or 0.
extern "C" uintptr_t rsf_skyrim_session_lease(bool live, rsf_game_host_services* out) noexcept;
extern "C" void rsf_skyrim_session_withdraw() noexcept;
