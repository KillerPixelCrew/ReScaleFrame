/* SPDX-License-Identifier: MIT */
/* Typed native render continuations and foreign exception guards.
   MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the same
   exceptions the former catch-all stopped. Access violations and other faults still propagate.
   Activity and TAA restore scopes use __try/__finally where C++ destructors closed them on unwind. */
#include <rescaleframe/game_api.h>
#include <windows.h>
#include "cpu_call.h"
#include "observer_platform.h"
#include "postprocess_platform.h"
#include "render_observer_platform.h"
#include "session_platform.h"
#include <rescaleframe/d3d11_host.h>

#if !defined(_MSC_VER)
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif
#include <intrin.h>

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL
#define RSF_FOREIGN_EXCEPTION \
    (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER : EXCEPTION_CONTINUE_SEARCH)

bool rsf_skyrim_resolution_controls(uintptr_t state);
void rsf_skyrim_observer_enter(void);
void rsf_skyrim_observer_leave(void);

bool rsf_skyrim_log_guarded(const rsf_game_host_services* host, const char* message)
{
    __try {
        host->log(host->user, message);
        return true;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return false;
    }
}

bool rsf_skyrim_render_pass_guarded(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass, uint32_t begin)
{
    __try {
        host->render_pass(host->user, context, pass, begin);
        return true;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return false;
    }
}

int32_t rsf_skyrim_upscale_guarded(const rsf_game_host_services* host, void* context,
    const rsf_game_render_pass* pass)
{
    __try {
        return host->process_upscale(host->user, context, pass);
    } __except (RSF_FOREIGN_EXCEPTION) {
        return RSF_ERROR_NOT_READY;
    }
}

bool rsf_skyrim_render_config_guarded(const rsf_game_host_services* host, rsf_game_render_config* config)
{
    __try {
        (void)host->render_config(host->user, config);
        return true;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return false;
    }
}

/* Keep CS's extra fourth argument and its fifth stack byte in the native signature. */
typedef uintptr_t (*process_main_fn)(void*, uint32_t, uint32_t, uintptr_t, uint8_t);
typedef void (*update_jitter_fn)(void*);
typedef uintptr_t (*renderer_end_fn)(void);
typedef void (*upload_frame_fn)(void*, uint8_t);

/* The activity counter must drop even when the engine continuation raises. */
#define RSF_OBSERVER_ACTIVITY_TRY rsf_skyrim_observer_enter(); __try {
#define RSF_OBSERVER_ACTIVITY_END } __finally { rsf_skyrim_observer_leave(); }

uintptr_t rsf_skyrim_observe_process(void* manager, uint32_t input, uint32_t output, uintptr_t argument4,
    uint8_t argument5)
{
    const uintptr_t caller = (uintptr_t)_ReturnAddress();
    rsf_skyrim_taa_receipt receipt = {0};
    uintptr_t result = 0;
    RSF_OBSERVER_ACTIVITY_TRY
        __try {
            const rsf_skyrim_cs_hdr_call call = {caller, (uintptr_t)manager, argument4, input, output, argument5};
            rsf_skyrim_postprocess_prepare(&call, &receipt);
            result = ((process_main_fn)rsf_skyrim_observer_original(RSF_SKYRIM_OBSERVER_PROCESS))(
                manager, input, output, argument4, argument5);
        } __finally {
            rsf_skyrim_postprocess_restore(&receipt);
        }
    RSF_OBSERVER_ACTIVITY_END
    return result;
}

void rsf_skyrim_observe_jitter(void* state)
{
    RSF_OBSERVER_ACTIVITY_TRY
        ((update_jitter_fn)rsf_skyrim_observer_original(RSF_SKYRIM_OBSERVER_JITTER))(state);
        rsf_skyrim_render_jitter_observed((uintptr_t)state);
    RSF_OBSERVER_ACTIVITY_END
}

void rsf_skyrim_observe_resolution(void* state)
{
    RSF_OBSERVER_ACTIVITY_TRY
        if (!rsf_skyrim_resolution_controls((uintptr_t)state))
            ((update_jitter_fn)rsf_skyrim_observer_original(RSF_SKYRIM_OBSERVER_RESOLUTION))(state);
    RSF_OBSERVER_ACTIVITY_END
}

uintptr_t rsf_skyrim_observe_end(void)
{
    rsf_game_host_services host = {0};
    uintptr_t result = 0;
    RSF_OBSERVER_ACTIVITY_TRY
        rsf_skyrim_render_end_enter(&host);
        result = ((renderer_end_fn)rsf_skyrim_observer_original(RSF_SKYRIM_OBSERVER_END))();
        rsf_skyrim_render_end_leave(&host);
    RSF_OBSERVER_ACTIVITY_END
    return result;
}

void rsf_skyrim_observe_upload(void* renderer, uint8_t flags)
{
    rsf_skyrim_upload_observation observation = {0};
    RSF_OBSERVER_ACTIVITY_TRY
        const bool uploading = rsf_skyrim_render_upload_enter((uintptr_t)renderer, &observation);
        ((upload_frame_fn)rsf_skyrim_observer_original(RSF_SKYRIM_OBSERVER_UPLOAD))(renderer, flags);
        if (uploading) rsf_skyrim_render_upload_leave(&observation, flags);
    RSF_OBSERVER_ACTIVITY_END
}

bool rsf_skyrim_host_create_guarded(uintptr_t address, const rsf_d3d11_host_setup* setup,
    rsf_game_host_services* services, int32_t* result)
{
    __try {
        *result = ((int32_t (*)(const rsf_d3d11_host_setup*, rsf_game_host_services*))address)(setup, services);
        return true;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return false;
    }
}

bool rsf_skyrim_host_active_guarded(uintptr_t address, void* user, uint32_t active, int32_t* result)
{
    __try {
        *result = ((int32_t (*)(void*, uint32_t))address)(user, active);
        return true;
    } __except (RSF_FOREIGN_EXCEPTION) {
        return false;
    }
}
