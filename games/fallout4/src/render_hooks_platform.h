// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between the typed render detours (render_hooks.c) and their Rust owner
// (render_hooks.rs), which keeps the World and every decision. Not a DLL ABI.
#include "render_records.h"
#include <rescaleframe/game_renderer.h>

typedef struct rsf_d3d11_reduced_frame rsf_d3d11_reduced_frame;
typedef struct rsf_d3d11_host_capture rsf_d3d11_host_capture;
typedef struct rsf_d3d11_auxiliary_pass rsf_d3d11_auxiliary_pass;
typedef struct rsf_d3d11_auxiliary_scope rsf_d3d11_auxiliary_scope;

// Detours in creation and enable order. Rust owns each record's RVA, expected entry bytes and
// created state; render_hooks.c owns the typed detour and its typed trampoline.
enum {
    RenderHookDynamicResolution = 0, // void(void*, void*, void*, void*, void*)
    RenderHookPrepass = 1,           // void()
    RenderHookNativePresent = 2,     // void(void* renderer)
    RenderHookWorldBegin = 3,        // void()
    RenderHookViewport = 4,          // void(void* manager, bool)
    RenderHookImagespace = 5,        // void()
    RenderHookEffects = 6,           // void(void*, uint32_t, uint32_t, uint32_t, uint32_t)
    RenderHookTaa = 7,               // bool(void* effect)
    RenderHookCount = 8
};

bool rsf_fallout4_render_create_hook(uint32_t hook, void* target);
bool rsf_fallout4_render_remove_hook(uint32_t hook, void* target);
// A detour is in flight.
bool rsf_fallout4_render_entered(void);

// The World source before the original prepass and its selection, held across the original.
typedef struct NativePrepassCall {
    rsf_fallout4_cpu_identity identity;
    bool selected;
} NativePrepassCall;
RSF_FALLOUT4_RENDER_ASSERT(sizeof(NativePrepassCall) == 48 && offsetof(NativePrepassCall, selected) == 40);

// The Rust seams each detour calls around its original.
bool rsf_fallout4_render_begin_world(uintptr_t caller, bool ordinary_world);
void rsf_fallout4_render_after_viewport(uintptr_t caller, uintptr_t manager, bool enabled);
void rsf_fallout4_render_before_dynamic_resolution(void);
void rsf_fallout4_render_after_dynamic_resolution(uintptr_t caller, uintptr_t manager);
void rsf_fallout4_render_before_prepass(uintptr_t caller, uint32_t depth, NativePrepassCall*);
void rsf_fallout4_render_after_prepass(const NativePrepassCall*);
bool rsf_fallout4_render_enter_imagespace(uintptr_t caller);
void rsf_fallout4_render_leave_imagespace(void);
bool rsf_fallout4_render_before_effects(uintptr_t caller, uintptr_t manager, uint32_t first, uint32_t last,
    uint32_t mode, uint32_t flags, rsf_fallout4_cpu_identity* admitted);
void rsf_fallout4_render_after_effects(const rsf_fallout4_cpu_identity* admitted);
bool rsf_fallout4_render_gate_taa(bool original);
bool rsf_fallout4_render_before_present(uintptr_t caller, uintptr_t renderer, rsf_fallout4_cpu_identity* phase);
void rsf_fallout4_render_after_present(const rsf_fallout4_cpu_identity* phase);

// Runtime calls behind the foreign-exception guard. False when the callee threw.
typedef int32_t (*rsf_fallout4_upscale_auxiliaries_fn)(void*, void*, const rsf_d3d11_auxiliary_pass*);
typedef int32_t (*rsf_fallout4_restore_auxiliaries_fn)(void*, const rsf_d3d11_auxiliary_scope*);
typedef int32_t (*rsf_fallout4_reduced_frame_fn)(void*, void*, rsf_d3d11_reduced_frame*);
typedef int32_t (*rsf_fallout4_capture_texture_fn)(void*, void*, const rsf_d3d11_host_capture*);
typedef int (*rsf_fallout4_capture_requested_fn)(void);
bool rsf_fallout4_guard_notify(rsf_game_render_pass_fn, void* user, void* context, const rsf_game_render_pass*,
    uint32_t begin);
bool rsf_fallout4_guard_upscale(rsf_game_process_upscale_fn, void* user, void* context, const rsf_game_render_pass*,
    int32_t* result);
bool rsf_fallout4_guard_auxiliaries(rsf_fallout4_upscale_auxiliaries_fn, void* user, void* context,
    const rsf_d3d11_auxiliary_pass*, int32_t* result);
bool rsf_fallout4_guard_restore(rsf_fallout4_restore_auxiliaries_fn, void* user, const rsf_d3d11_auxiliary_scope*,
    int32_t* result);
bool rsf_fallout4_guard_reduced(rsf_fallout4_reduced_frame_fn, void* user, void* context, rsf_d3d11_reduced_frame*,
    int32_t* result);
bool rsf_fallout4_guard_capture(rsf_fallout4_capture_texture_fn, void* user, void* context,
    const rsf_d3d11_host_capture*);
bool rsf_fallout4_guard_requested(rsf_fallout4_capture_requested_fn, bool* requested);
