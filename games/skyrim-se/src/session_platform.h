// SPDX-License-Identifier: MIT
#pragma once
// Internal boundary between plugin.cpp and the Rust session owner (src/session.rs, crate
// rsf-skyrim-se). Not a DLL ABI: both sides link into ReScaleFrame.Game.SkyrimSE.dll.
// src/session.rs mirrors the private leaves. The public setup comes from the generated SDK binding.
// Rust owns the lifecycle order, results,
// reasons, the Runtime host setup and its original-session reuse, DPI startup and the borrowed
// services. The leaves keep the C++ subsystems, the detours and their engine facts.
#include "observer_platform.h"

#define RSF_SKYRIM_SESSION_LEAVES_VERSION 3u

// Native subsystem operations (plugin.cpp). value is 0 or 1 unless stated. services is read only
// by the operations marked "services".
#define RSF_SKYRIM_ADMIT_OBSERVERS 0u      /* 1 admitted, 0 refused, negative unavailable */
#define RSF_SKYRIM_OBSERVERS_BUSY 1u       /* an observer detour is in flight */
#define RSF_SKYRIM_RESET_OBSERVATIONS 2u   /* value is always 1 */
#define RSF_SKYRIM_CPU_PREPARE 3u
#define RSF_SKYRIM_CPU_START 4u
#define RSF_SKYRIM_CPU_QUIESCE 5u          /* value is always 1 */
#define RSF_SKYRIM_CPU_DISABLE 6u
#define RSF_SKYRIM_CPU_STOP 7u
#define RSF_SKYRIM_CPU_BUSY 8u
#define RSF_SKYRIM_SAMPLING_START 9u       /* services */
#define RSF_SKYRIM_SAMPLING_QUIESCE 10u    /* value is always 1 */
#define RSF_SKYRIM_SAMPLING_STOP 11u
#define RSF_SKYRIM_SAMPLING_BUSY 12u
#define RSF_SKYRIM_PRESENTATION_START 13u  /* services */
#define RSF_SKYRIM_PRESENTATION_QUIESCE 14u /* value is always 1 */
#define RSF_SKYRIM_PRESENTATION_STOP 15u
#define RSF_SKYRIM_PRESENTATION_BUSY 16u
#define RSF_SKYRIM_RESOLUTION_OWNED 17u
#define RSF_SKYRIM_AUXILIARIES_PENDING 18u
#define RSF_SKYRIM_CPU_BIND 19u             /* services */

// Every guarded leaf returns false only when its native call threw; its result goes to the out
// parameter. The table lives for the process.
struct rsf_skyrim_session_leaves {
    uint32_t struct_size, version;
    const rsf_skyrim_creation_platform* platform;
    const rsf_skyrim_observer_leaves* observers;
    bool (*subsystem)(uint32_t operation, const rsf_game_host_services* services, int32_t* value);
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
static_assert(sizeof(rsf_skyrim_session_leaves) == 8 + 10 * sizeof(void*));
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
