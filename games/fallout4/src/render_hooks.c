/* SPDX-License-Identifier: MIT */
/* The typed render detours of the Rust render hook owner (render_hooks.rs) and the guarded Runtime
   calls it makes. A detour calls its original on its own C frame, so a native C++ unwind crosses only
   these shells; each runs its exit work in a termination handler. Rust keeps the World and decides. */
#include "render_hooks_platform.h"
#include "native_guard.h"
#include <rescaleframe/d3d11_host.h>
#include <MinHook.h>

typedef void (*Viewport)(void*, bool);
typedef void (*Imagespace)(void);
typedef void (*WorldBegin)(void);
typedef void (*Effects)(void*, uint32_t, uint32_t, uint32_t, uint32_t);
typedef bool (*TaaActive)(void*);
typedef void (*NativePresent)(void*);
typedef void (*Prepass)(void);
typedef void (*DynamicResolution)(void*, void*, void*, void*, void*);

/* The trampolines MinHook writes at creation and clears at removal. */
static Viewport viewport;
static Imagespace imagespace;
static WorldBegin world_begin;
static Effects effects;
static TaaActive taa_active;
static NativePresent native_present;
static Prepass prepass;
static DynamicResolution dynamic_resolution;

static volatile LONG entered;
RSF_FALLOUT4_THREAD_LOCAL static bool ordinary_world;
RSF_FALLOUT4_THREAD_LOCAL static uint32_t prepass_depth;

static void observe_viewport(void* manager, bool enabled)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        viewport(manager, enabled);
        const DWORD error = GetLastError();
        rsf_fallout4_render_after_viewport(caller, (uintptr_t)manager, enabled);
        SetLastError(error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static void observe_world_begin(void)
{
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    const bool previous = ordinary_world;
    RSF_FOREIGN_TRY {
        if (rsf_fallout4_render_begin_world(caller, ordinary_world)) ordinary_world = true;
        SetLastError(error);
        world_begin();
    }
    RSF_FOREIGN_FINALLY {
        ordinary_world = previous;
        InterlockedDecrement(&entered);
    }
}

static void observe_dynamic_resolution(void* manager, void* a, void* b, void* c, void* d)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const DWORD error = GetLastError();
        rsf_fallout4_render_before_dynamic_resolution();
        SetLastError(error);
        dynamic_resolution(manager, a, b, c, d);
        const DWORD native_error = GetLastError();
        rsf_fallout4_render_after_dynamic_resolution(caller, (uintptr_t)manager);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static void observe_prepass(void)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    ++prepass_depth;
    RSF_FOREIGN_TRY {
        NativePrepassCall call = {0};
        rsf_fallout4_render_before_prepass(caller, prepass_depth, &call);
        SetLastError(error);
        prepass();
        const DWORD native_error = GetLastError();
        rsf_fallout4_render_after_prepass(&call);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY {
        --prepass_depth;
        InterlockedDecrement(&entered);
    }
}

static void observe_imagespace(void)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    const bool selected = rsf_fallout4_render_enter_imagespace(caller);
    RSF_FOREIGN_TRY {
        SetLastError(error);
        imagespace();
    }
    RSF_FOREIGN_FINALLY {
        if (selected) {
            const DWORD exit_error = GetLastError();
            rsf_fallout4_render_leave_imagespace();
            SetLastError(exit_error);
        }
        InterlockedDecrement(&entered);
    }
}

static void observe_effects(void* manager, uint32_t first, uint32_t last, uint32_t mode, uint32_t flags)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const DWORD error = GetLastError();
        rsf_fallout4_cpu_identity admitted = {0};
        const bool selected = rsf_fallout4_render_before_effects(caller, (uintptr_t)manager, first, last, mode,
            flags, &admitted);
        SetLastError(error);
        effects(manager, first, last, mode, flags);
        const DWORD native_error = GetLastError();
        if (selected) rsf_fallout4_render_after_effects(&admitted);
        SetLastError(native_error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
}

static bool observe_taa(void* effect)
{
    bool active = false;
    InterlockedIncrement(&entered);
    RSF_FOREIGN_TRY {
        const bool result = taa_active(effect);
        const DWORD error = GetLastError();
        active = rsf_fallout4_render_gate_taa(result);
        SetLastError(error);
    }
    RSF_FOREIGN_FINALLY { InterlockedDecrement(&entered); }
    return active;
}

static void observe_native_present(void* renderer)
{
    const uintptr_t caller = RSF_FALLOUT4_CALLER();
    InterlockedIncrement(&entered);
    const DWORD error = GetLastError();
    /* Close the same CPU phase after the original, even if it unwinds. */
    rsf_fallout4_cpu_identity phase = {0};
    volatile bool phase_active = false;
    RSF_FOREIGN_TRY {
        phase_active = rsf_fallout4_render_before_present(caller, (uintptr_t)renderer, &phase);
        SetLastError(error);
        native_present(renderer);
    }
    RSF_FOREIGN_FINALLY {
        if (phase_active) {
            const DWORD exit_error = GetLastError();
            rsf_fallout4_render_after_present(&phase);
            SetLastError(exit_error);
        }
        InterlockedDecrement(&entered);
    }
}

static void* render_detour(uint32_t hook)
{
    switch (hook) {
    case RenderHookDynamicResolution: return (void*)&observe_dynamic_resolution;
    case RenderHookPrepass: return (void*)&observe_prepass;
    case RenderHookNativePresent: return (void*)&observe_native_present;
    case RenderHookWorldBegin: return (void*)&observe_world_begin;
    case RenderHookViewport: return (void*)&observe_viewport;
    case RenderHookImagespace: return (void*)&observe_imagespace;
    case RenderHookEffects: return (void*)&observe_effects;
    case RenderHookTaa: return (void*)&observe_taa;
    default: return NULL;
    }
}

static void** render_original(uint32_t hook)
{
    switch (hook) {
    case RenderHookDynamicResolution: return (void**)&dynamic_resolution;
    case RenderHookPrepass: return (void**)&prepass;
    case RenderHookNativePresent: return (void**)&native_present;
    case RenderHookWorldBegin: return (void**)&world_begin;
    case RenderHookViewport: return (void**)&viewport;
    case RenderHookImagespace: return (void**)&imagespace;
    case RenderHookEffects: return (void**)&effects;
    case RenderHookTaa: return (void**)&taa_active;
    default: return NULL;
    }
}

bool rsf_fallout4_render_create_hook(uint32_t hook, void* target)
{
    return hook < RenderHookCount && MH_CreateHook(target, render_detour(hook), render_original(hook)) == MH_OK;
}

bool rsf_fallout4_render_remove_hook(uint32_t hook, void* target)
{
    if (hook >= RenderHookCount || MH_RemoveHook(target) != MH_OK) return false;
    *render_original(hook) = NULL;
    return true;
}

bool rsf_fallout4_render_entered(void)
{
    return InterlockedCompareExchange(&entered, 0, 0) != 0;
}

bool rsf_fallout4_guard_notify(rsf_game_render_pass_fn notify, void* user, void* context,
    const rsf_game_render_pass* pass, uint32_t begin)
{
    bool returned = true;
    RSF_FOREIGN_TRY { notify(user, context, pass, begin); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_upscale(rsf_game_process_upscale_fn upscale, void* user, void* context,
    const rsf_game_render_pass* pass, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = upscale(user, context, pass); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_auxiliaries(rsf_fallout4_upscale_auxiliaries_fn upscale, void* user, void* context,
    const rsf_d3d11_auxiliary_pass* request, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = upscale(user, context, request); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_restore(rsf_fallout4_restore_auxiliaries_fn restore, void* user,
    const rsf_d3d11_auxiliary_scope* scope, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = restore(user, scope); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_reduced(rsf_fallout4_reduced_frame_fn reduce, void* user, void* context,
    rsf_d3d11_reduced_frame* request, int32_t* result)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *result = reduce(user, context, request); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_capture(rsf_fallout4_capture_texture_fn capture, void* user, void* context,
    const rsf_d3d11_host_capture* request)
{
    bool returned = true;
    RSF_FOREIGN_TRY { (void)capture(user, context, request); }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}

bool rsf_fallout4_guard_requested(rsf_fallout4_capture_requested_fn query, bool* requested)
{
    bool returned = true;
    RSF_FOREIGN_TRY { *requested = query() != 0; }
    RSF_FOREIGN_CATCH { returned = false; }
    return returned;
}
