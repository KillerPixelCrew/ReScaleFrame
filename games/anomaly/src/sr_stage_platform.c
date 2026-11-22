/* SPDX-License-Identifier: MIT */
/* Native leaves of the SR stage driver. sr_stage.rs owns the lifecycle, admission, ordering, configuration and
   status; sr_stage/native.rs builds the production leaf table over the Rust profile records. This file keeps what Rust
   cannot express: the five stage trampolines and the position-binder slot replacement, the SEH calls into the engine,
   the guarded memory copy, the C++ exception boundary around foreign log sinks, the hook set and the C entries. */
#include "sr_stage.h"
#include "sr_stage_platform.h"
#include "engine_slots.h"
#include "native_geometry.h"
#include "native_hooks.h"
#include "native_memory.h"
#include "sr_d3d11_platform.h"
#include "stage_entries.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <string.h>

_Static_assert(sizeof(rsf_anomaly_hook_site) == 48 && offsetof(rsf_anomaly_hook_site, original) == 40,
    "hook site layout lent by Rust");
_Static_assert(sizeof(void*) == 8, "x64 only");

/* ---- the stage trampolines ------------------------------------------------------------------------------------ */
typedef void (__fastcall *unary_fn)(void*);
typedef void (__fastcall *setup_fn)(void*, void*);
enum { PREPARE, COMBINE, SUNSHAFTS, BLUR, SUN_CASCADES };
static void* originals[RSF_ANOMALY_SR_DETOUR_COUNT];
static rsf_anomaly_hook_set hooks;

static void __fastcall prepare_hook(void* self)
{
    rsf_anomaly_sr_stage_prepare(self);
    ((unary_fn)originals[PREPARE])(self);
    rsf_anomaly_sr_stage_prepared();   /* Stage 2: the viewport now describes the render-size targets */
}

static void __fastcall combine_hook(void* self)
{
    ((unary_fn)originals[COMBINE])(self);
    rsf_anomaly_sr_stage_combine_end();
}

static void __fastcall sunshafts_hook(void* self)
{
    rsf_anomaly_sr_stage_seam(self);
    ((unary_fn)originals[SUNSHAFTS])(self);
}

static void __fastcall blur_hook(void* self)
{
    rsf_anomaly_sr_stage_seam(self);
    ((unary_fn)originals[BLUR])(self);
}

/* The original can re-enter draw callbacks, so the stage lock is held only while changing the camera. The camera is
   restored on return and also when an engine C++ exception leaves the original: by the termination handler under
   MSVC, by the cleanup handler under GCC (built with -fexceptions). */
#ifdef _MSC_VER
static void __fastcall sun_cascades_hook(void* self)
{
    const bool suspended = rsf_anomaly_sr_stage_sun_begin();
    __try {
        ((unary_fn)originals[SUN_CASCADES])(self);
    } __finally {
        rsf_anomaly_sr_stage_sun_end(suspended);
    }
}
#else
static void end_sun(const bool* suspended)
{
    rsf_anomaly_sr_stage_sun_end(*suspended);
}
static void __fastcall sun_cascades_hook(void* self)
{
    const bool suspended __attribute__((cleanup(end_sun))) = rsf_anomaly_sr_stage_sun_begin();
    ((unary_fn)originals[SUN_CASCADES])(self);
}
#endif

/* cl_pos_decompress_params::setup is reached only through the binder's vtable (R_constant_setup::setup, slot 0). The
   AVX body has a VEX instruction seven bytes in, which MinHook cannot relocate, so both builds swap the slot. */
static void __fastcall position_setup_hook(void* self, void* constant)
{
    ((setup_fn)rsf_anomaly_slot_original(RSF_ANOMALY_POSITION_SLOT))(self, constant);
    if (rsf_anomaly_position_slot_active()) rsf_anomaly_sr_stage_position_setup(constant);
}

static bool install_hooks(void* image, uint32_t size, rsf_anomaly_hook_site* sites, size_t count)
{
    static void* const detours[RSF_ANOMALY_SR_DETOUR_COUNT] = {
        (void*)&prepare_hook, (void*)&combine_hook, (void*)&sunshafts_hook, (void*)&blur_hook,
        (void*)&sun_cascades_hook,
    };
    if (!sites || count != RSF_ANOMALY_SR_DETOUR_COUNT) return false;
    for (size_t i = 0; i < count; ++i) {
        sites[i].replacement = detours[i];
        sites[i].original = &originals[i];
    }
    return rsf_anomaly_hooks_install(&hooks, image, size, sites, count);
}
static bool hooks_created(void) { return rsf_anomaly_hooks_created(&hooks); }
static bool enable_hooks(void) { return rsf_anomaly_hooks_enable(&hooks); }
static bool disable_hooks(void) { return rsf_anomaly_hooks_disable(&hooks); }

/* ---- guarded engine calls (byte-guarded at install, x64 __fastcall) ------------------------------------------- */
typedef void (__fastcall *set_projection_fn)(void* xforms, const void* matrix);
typedef void (__fastcall *set_mip_bias_fn)(void* cache, float bias);
typedef unsigned char* (__fastcall *constant_buffer_fn)(void* constants, void* constant, uint32_t stage);

static bool set_projection(void* function, void* xforms, const void* matrix)
{
#ifdef _MSC_VER
    __try {
        ((set_projection_fn)function)(xforms, matrix);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
#else
    ((set_projection_fn)function)(xforms, matrix);
    return true;
#endif
}

static bool set_mip_bias(void* function, void* cache, float bias)
{
#ifdef _MSC_VER
    __try {
        ((set_mip_bias_fn)function)(cache, bias);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
#else
    ((set_mip_bias_fn)function)(cache, bias);
    return true;
#endif
}

static uintptr_t constant_buffer(void* function, void* constants, uintptr_t constant, uint32_t stage)
{
#ifdef _MSC_VER
    __try {
        return (uintptr_t)((constant_buffer_fn)function)(constants, (void*)constant, stage);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return 0;
    }
#else
    return (uintptr_t)((constant_buffer_fn)function)(constants, (void*)constant, stage);
#endif
}

/* ---- memory, logs and helpers ---------------------------------------------------------------------------------- */
static bool copy(void* output, const void* input, size_t size) { return guarded_copy(output, input, size); }

static const void* d3d11(void) { return rsf_anomaly_sr_d3d11_platform(); }

static const rsf_anomaly_sr_native native = {
    sizeof(rsf_anomaly_sr_native),
    copy,
    guarded_log_line,
    set_projection,
    set_mip_bias,
    constant_buffer,
    install_hooks,
    hooks_created,
    enable_hooks,
    disable_hooks,
    (void*)&position_setup_hook,
    rsf_anomaly_compare_slot,
    rsf_anomaly_geometry_current_view,
    rsf_texture_pixel_bytes,
    rsf_d3d11_create_shader_view,
    d3d11,
};

const rsf_anomaly_sr_native* rsf_anomaly_sr_native_leaves(void) { return &native; }

/* ---- C entries for the bootstrap and observer leaves (stage_entries.h) ---------------------------------------- */
_Static_assert(sizeof(wchar_t) == sizeof(uint16_t), "UTF-16 INI path");

bool rsf_anomaly_sr_configure_native(const wchar_t* ini, bool apply, rsf_game_log_fn log)
{
    return rsf_anomaly_sr_stage_configure_native(&native, (const uint16_t*)ini, apply, log, NULL);
}

bool rsf_anomaly_sr_install_native(void* image, size_t build)
{
    return rsf_anomaly_sr_stage_install_build(&native, image, build);
}
