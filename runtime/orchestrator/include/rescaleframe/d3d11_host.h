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
/* Graphics owner only, after process_upscale and before the matching actual Present
   completes. Later native callbacks must preserve the same frame/view identity.
   Adds an adapter-selected texture to an already requested capture. No capture
   is started here. Full session/native/view identity must match; borrowed
   immediate context and texture are not retained. Refusal before an eligible
   capture does not dereference either graphics pointer. GPU readback stays in
   the runtime; native resource selection and meaning stay in the adapter. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_capture_texture(void* services_user, void* immediate_context,
    const rsf_d3d11_host_capture*);

#define RSF_D3D11_AUXILIARY_DEPTH_STENCIL 0u
#define RSF_D3D11_AUXILIARY_COLOUR 1u
#define RSF_D3D11_AUXILIARY_RESTORE_CONTENTS 0x1u
typedef struct rsf_d3d11_auxiliary_surface {
    uint32_t struct_size, kind;
    void* texture;
    /* Source viewport bounds may be fractional. Depth/stencil requires integer bounds.
       Destination bounds are integer pixel edges. Samples subtract source-pixel jitter. */
    float source_rect[4];
    int32_t output_rect[4];
    float jitter_pixels[2];
    const char* name; /* Optional capture name, describing the reconstructed auxiliary. */
    const char* source_name; /* Optional capture name for original auxiliary contents. */
    /* Restore original contents after the final consumer when the owner requires
       reduced history. Zero preserves later native writes, including clears.
       Failed SR always rolls back every modified auxiliary regardless of this flag. */
    uint32_t flags;
} rsf_d3d11_auxiliary_surface;
typedef struct rsf_d3d11_auxiliary_pass {
    uint32_t struct_size, abi_version;
    const rsf_game_render_pass* pass;
    uint32_t surface_count;
    const rsf_d3d11_auxiliary_surface* surfaces;
} rsf_d3d11_auxiliary_pass;
typedef struct rsf_d3d11_auxiliary_scope {
    uint32_t struct_size, abi_version;
    uint64_t session_id, native_frame, view_key;
} rsf_d3d11_auxiliary_scope;
/* One synchronous SR transaction on the admitted graphics owner. Prepares all
   auxiliaries without native writes, evaluates SR, then commits auxiliaries.
   On failure restores native colour and auxiliaries for the native fallback.
   Success leases auxiliary contents through restore_auxiliaries, after the last
   native consumer and before the next producer. Each surface's flags specify
   content restoration or retirement preserving native writes. Identity/thread must match.
   The adapter supplies resources/rectangles/jitter and native consumer lifetime;
   shader work, copies, snapshots and binding restoration belong to the runtime. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_upscale_auxiliaries(void* services_user, void* immediate_context,
    const rsf_d3d11_auxiliary_pass*);
/* Restoration is permitted while the host is suspended so quiesce can drain. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_restore_auxiliaries(void* services_user,
    const rsf_d3d11_auxiliary_scope*);

typedef struct rsf_d3d11_sampler_bias {
    uint32_t struct_size, abi_version;
    uint64_t session_id;
    /* Native baseline PS samplers, borrowed for this call. Only selected slots
       receive additive LOD bias. Unselected slots restore owned replacements.
       A binding foreign to both baseline and this service is left untouched. */
    void* samplers[16];
    uint32_t slot_mask;
    float bias;
    uint32_t applied_mask; /* Output: slots still carrying an owned replacement. */
} rsf_d3d11_sampler_bias;
/* Admitted graphics owner only. Adapter supplies scene scope and sampling policy;
   the runtime owns a bounded cache of immutable sampler objects. Zero mask drains. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_bind_sampler_bias(void* services_user, void* immediate_context,
    rsf_d3d11_sampler_bias*);
#ifdef __cplusplus
}
#endif
