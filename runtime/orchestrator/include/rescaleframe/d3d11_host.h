// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/motion_decode.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_D3D11_HOST_ABI_VERSION 1u
typedef struct rsf_d3d11_host_setup {
    uint32_t struct_size, abi_version;
    const wchar_t* configuration_path;
    const wchar_t* configuration_section;
    const wchar_t* runtime_directory;
    const char* identity_utf8;
    const char* executable_sha256;
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    float units_to_meters;
    uint32_t depth_inverted;
    rsf_motion_decode_params motion;
    /* Adapter capability: bit n accepts quality n. Native-only adapters supply bit 0. */
    uint32_t quality_choices;
} rsf_d3d11_host_setup;
/* Graphics-free setup, outside DllMain. Input services supply session/log/CPU only.
   Strings are copied; upstream callback code/user and returned services are process-lived.
   The adapter selects and guards its native hooks before invoking this service.
   process_upscale borrows immediate-context textures synchronously and restores bindings.
   The host owns SDK selection/settings/overlay/capture. It does not infer engine inputs.
   This SR host does not authorize generation or pacing from render-only identities. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup*, rsf_game_host_services*);
/* Any thread. Suspend callbacks during adapter quiescence; no graphics retirement occurs here.
   The returned host is process-lived and starts suspended. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_set_active(void* services_user, uint32_t active);
/* Any thread. Arms one bounded capture at the next eligible graphics callback. */
RSF_RUNTIME_API uint32_t rsf_d3d11_host_request_capture(void* unused);
#ifdef __cplusplus
}
#endif
