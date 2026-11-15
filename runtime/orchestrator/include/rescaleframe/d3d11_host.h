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
   RECONFIGURE on the graphics owner prepares dimensions without reconstruction; query render_config
   afterwards. Output dimensions and swapchain are required; camera_valid may be zero for planning.
   Presentation binding may initialize window/UI observation before a scene exists.
   The first verified scene lease separately admits SR preparation; earlier RECONFIGURE requests are refused.
   The host owns SDK selection/settings/overlay/capture. It does not infer engine inputs.
   This SR host does not authorize generation or pacing from render-only identities. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup*, rsf_game_host_services*);
/* Any thread. Suspend callbacks during adapter quiescence; no graphics retirement occurs here.
   The returned host is process-lived and starts suspended. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_set_active(void* services_user, uint32_t active);
typedef struct rsf_d3d11_presentation_binding {
    uint32_t struct_size, abi_version;
    void* swapchain;
} rsf_d3d11_presentation_binding;
/* Native presentation owner, once its actual D3D11 chain exists, before Present.
   Binds window/input/overlay observation independently of scene admission. No SR
   vendor SDK is started here. The adapter owns the guarded native call site;
   the runtime owns graphics, window subclassing and shared UI. Repeated binding
   on the same chain/owner is cheap. Serialized startup owner transfers are
   allowed before scene/SR admission. Device/chain recreation is not admitted. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_bind_presentation(void* services_user,
    const rsf_d3d11_presentation_binding*);
/* Any thread. Arms one bounded capture at the next eligible graphics callback. */
RSF_RUNTIME_API uint32_t rsf_d3d11_host_request_capture(void* unused);

typedef struct rsf_d3d11_host_capture {
    uint32_t struct_size, abi_version;
    uint64_t session_id, native_frame, view_key;
    const char* name;
    const char* description;
    void* texture;
    uint32_t view; /* rsf_dump_view: 0 storage-range, 3 linear HDR, 4 depth preview. */
    float scale;
} rsf_d3d11_host_capture;
/* Graphics owner only, immediately after process_upscale for the same pass.
   Adds an adapter-selected texture to an already requested capture. No capture
   is started here. Full session/native/view identity must match; borrowed
   immediate context and texture are not retained. Refusal before an eligible
   capture does not dereference either graphics pointer. GPU readback stays in
   the runtime; native resource selection and meaning stay in the adapter. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_capture_texture(void* services_user, void* immediate_context,
    const rsf_d3d11_host_capture*);
#ifdef __cplusplus
}
#endif
