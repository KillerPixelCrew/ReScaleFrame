/* SPDX-License-Identifier: MIT */
/* Native leaves of the SR stage driver. sr_stage.rs owns the lifecycle, admission, ordering, configuration and
   status; sr_stage/native.rs calls these leaves by name over the Rust profile records and installs the detours. This
   file keeps what Rust cannot express: the five stage trampolines and the position-binder slot replacement and the SEH
   calls into the engine. */
#include "sr_stage.h"
#include "sr_stage_platform.h"
#include "engine_slots.h"
#include "native_geometry.h"
#include "native_memory.h"
#include <rescaleframe/d3d11_state.h>
#include <rescaleframe/texture_dump.h>
#include <windows.h>
#include <string.h>

_Static_assert(sizeof(void*) == 8, "x64 only");

/* ---- the stage trampolines ------------------------------------------------------------------------------------ */
typedef void (__fastcall *unary_fn)(void*);
typedef void (__fastcall *setup_fn)(void*, void*);
enum { PREPARE, COMBINE, SUNSHAFTS, BLUR, SUN_CASCADES };
/* MinHook writes the trampolines of the detours below (sr_stage/native.rs installs them). */
void* rsf_anomaly_sr_originals[RSF_ANOMALY_SR_DETOUR_COUNT];
#define originals rsf_anomaly_sr_originals

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
    RSF_FOREIGN_TRY {
        ((unary_fn)originals[SUN_CASCADES])(self);
    } RSF_FOREIGN_FINALLY {
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
void rsf_anomaly_sr_position_setup_detour(void* self, void* constant)
{
    ((setup_fn)rsf_anomaly_slot_original(RSF_ANOMALY_POSITION_SLOT))(self, constant);
    if (rsf_anomaly_position_slot_active()) rsf_anomaly_sr_stage_position_setup(constant);
}

void* const rsf_anomaly_sr_detours[RSF_ANOMALY_SR_DETOUR_COUNT] = {
    (void*)&prepare_hook, (void*)&combine_hook, (void*)&sunshafts_hook, (void*)&blur_hook, (void*)&sun_cascades_hook,
};
/* ---- guarded engine calls (byte-guarded at install, x64 __fastcall) ------------------------------------------- */
typedef void (__fastcall *set_projection_fn)(void* xforms, const void* matrix);
typedef void (__fastcall *set_mip_bias_fn)(void* cache, float bias);
typedef unsigned char* (__fastcall *constant_buffer_fn)(void* constants, void* constant, uint32_t stage);

bool rsf_anomaly_sr_native_set_projection(void* function, void* xforms, const void* matrix)
{
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        ((set_projection_fn)function)(xforms, matrix);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
#else
    ((set_projection_fn)function)(xforms, matrix);
    return true;
#endif
}

bool rsf_anomaly_sr_native_set_mip_bias(void* function, void* cache, float bias)
{
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        ((set_mip_bias_fn)function)(cache, bias);
        return true;
    } RSF_FAULT_CATCH {
        return false;
    }
#else
    ((set_mip_bias_fn)function)(cache, bias);
    return true;
#endif
}

uintptr_t rsf_anomaly_sr_native_constant_buffer(void* function, void* constants, uintptr_t constant, uint32_t stage)
{
#ifdef _MSC_VER
    RSF_FOREIGN_TRY {
        return (uintptr_t)((constant_buffer_fn)function)(constants, (void*)constant, stage);
    } RSF_FAULT_CATCH {
        return 0;
    }
#else
    return (uintptr_t)((constant_buffer_fn)function)(constants, (void*)constant, stage);
#endif
}


