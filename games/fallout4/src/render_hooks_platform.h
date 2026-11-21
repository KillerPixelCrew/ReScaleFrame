// SPDX-License-Identifier: MIT
#pragma once
// Private boundary between render_hooks.cpp and its Rust owner (render_hooks.rs): the render hook
// lifecycle and the ordinary World and outer Present guards. Not a DLL ABI.
#include "cpu_events_platform.h"
#include <cstddef>

namespace rsf::fallout4 {
// Detours in creation and enable order. Rust owns each record's RVA, expected entry bytes and
// created state; render_hooks.cpp owns the typed detour and its typed trampoline.
enum NativeRenderHook : uint32_t {
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

// Runtime exports Rust resolved by name, null when absent. C++ keeps them typed for its callbacks.
struct NativeRenderExports {
    void* upscale_auxiliaries;
    void* restore_auxiliaries;
    void* reduced_frame;
    void* capture_texture;
    void* capture_requested;
};

// Leaves the Rust lifecycle cannot express without MinHook or the C++ callback state. Constant
// for the process. Original engine functions are never called from Rust.
struct NativeRenderHookLeaves {
    // MH_CreateHook with the hook's detour, writing its typed trampoline.
    bool (*create_hook)(uint32_t hook, void* target) noexcept;
    // MH_RemoveHook; clears the typed trampoline only when removal succeeded.
    bool (*remove_hook)(uint32_t hook, void* target) noexcept;
    // MH_QueueEnableHook, MH_QueueDisableHook and MH_ApplyQueued, each true only for MH_OK.
    bool (*queue_enable_hook)(void* target) noexcept;
    bool (*queue_disable_hook)(void* target) noexcept;
    bool (*apply_queued)() noexcept;
    // Any detour in flight or any WINDOW, FRAME, auxiliary or recovery scope open.
    bool (*scopes_busy)() noexcept;
    // Replace the callbacks' Runtime exports and host services.
    void (*bind_exports)(const NativeRenderExports*) noexcept;
    void (*bind_host)(const rsf_game_host_services*) noexcept;
};

static_assert(sizeof(NativeRenderExports) == 5 * sizeof(void*));
static_assert(sizeof(NativeRenderHookLeaves) == 8 * sizeof(void*));

// The process-lived leaf table of render_hooks.cpp.
const NativeRenderHookLeaves* render_hook_leaves() noexcept;

struct NativeWorldAdmission {
    uintptr_t caller;
    uint64_t session;
    bool live, ordinary_world, frame_open, auxiliary_open, recovery_owned;
};

struct NativeRenderScopeKey {
    uint64_t frame, view, window;
    uintptr_t context, chain;
};

enum class NativePresentMatch : uint32_t { unrelated = 0, owner_changed = 1, matched = 2 };
enum class NativePresentAdmission : uint32_t { source_changed = 0, refused = 1, complete = 2 };

using ReadPresentOwner = bool (*)(NativeRenderScopeKey*, uintptr_t* outer_chain) noexcept;
using RevalidatePresentScreen = bool (*)() noexcept;

static_assert(sizeof(NativeWorldAdmission) == 24 && offsetof(NativeWorldAdmission, live) == 16 &&
    offsetof(NativeWorldAdmission, recovery_owned) == 20);
static_assert(sizeof(NativeRenderScopeKey) == 40 && offsetof(NativeRenderScopeKey, context) == 24 &&
    offsetof(NativeRenderScopeKey, chain) == 32);

extern "C" {
// Binds the leaves. The table must be complete and live for the process.
bool rsf_fallout4_render_publish(const NativeRenderHookLeaves*) noexcept;
// The adapter's render callbacks are live: set by start once every hook is queued, cleared by
// quiesce, stop and a failed start.
bool rsf_fallout4_render_live() noexcept;
bool rsf_fallout4_render_admit_world(const NativeWorldAdmission*, const rsf_fallout4_cpu_identity*) noexcept;
NativePresentMatch rsf_fallout4_render_match_present(uintptr_t caller, uintptr_t renderer,
    bool submission_open, const NativeRenderScopeKey*, ReadPresentOwner) noexcept;
NativePresentAdmission rsf_fallout4_render_admit_present(NativePresentMatch, uint64_t session,
    const rsf_fallout4_cpu_identity*, RevalidatePresentScreen) noexcept;
}
}
