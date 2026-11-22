// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <rescaleframe/runtime.h>
#include <rescaleframe/game_api.h>
#include <rescaleframe/motion_decode.h>
#include <rescaleframe/d3d11_chain_dispatch.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_D3D11_HOST_ABI_VERSION 8u
/* Implemented fact producers, not backend availability or per-frame acceptance. */
#define RSF_D3D11_HOST_CAP_FG_INPUTS 1u
#define RSF_D3D11_HOST_CAP_INPUT_TIMELINE 2u
#define RSF_D3D11_HOST_CAP_RENDERED_SOURCE 4u
/* ABI 8 size extension. The original setup prefix used depth_inverted for both flags. */
#define RSF_D3D11_HOST_DEPTH_FINITE 0u
#define RSF_D3D11_HOST_DEPTH_INFINITE 1u
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
    /* Optional cold presentation bridge. The adapter supplies native window
       selection; runtime switching starts with plain presentation and FG Off.
       Callback code/user remain valid for process lifetime. */
    /* 0 disabled, 1 factory interception, 2 late outer replacement,
       3 adapter-scoped factory interception below external wrappers. */
    uint32_t presentation_bridge;
    int (*accept_window)(void* user, void* window);
    void* presentation_user;
    /* FG_INPUTS: FG_INPUTS, FINAL_SCENE, WINDOW, FRAME and SUBMISSION leases.
       INPUT_TIMELINE: actual input consumption associated with source CPU facts.
       A declaration never bypasses native ownership, input or screen admission. */
    uint32_t capabilities;
    /* 0 preserves the enabled legacy default; 1 selects native colour (transport
       Off). An existing saved ColourTransport value always takes precedence. */
    uint32_t colour_transport_default;
    /* Optional, independent far-plane convention (RSF_D3D11_HOST_DEPTH_*). */
    uint32_t depth_far_plane;
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
   Generation and pacing require declared input capability and actual native eligibility;
   render-only identities never authorize them. */
/* Returned render_pass records adapter-declared FINAL_SCENE and WINDOW ownership.
   These synchronous native leases do not activate a generation provider. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_create(const rsf_d3d11_host_setup*, rsf_game_host_services*);
/* Any thread. Suspend callbacks during adapter quiescence; no graphics retirement occurs here.
   The returned host is process-lived and starts suspended. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_set_active(void* services_user, uint32_t active);

typedef struct rsf_d3d11_present_dispatch {
    uint32_t struct_size, abi_version;
    void* receiver;
    uint32_t sync_interval, flags;
    uint32_t handled;
    int32_t native_result;
} rsf_d3d11_present_dispatch;
/* Downstream continuation after external Present pre-work. The caller leases the
   receiver through this synchronous call. Only the exact registered inner facade
   is handled; no QI, dereference or result change occurs for an unowned receiver.
   A known but unavailable facade remains handled with a failing native_result.
   On RSF_OK, handled=0 delegates to the caller's current native continuation;
   handled=1 returns native_result, including failures, without further forwarding.
   A service error must not fall through to a native implementation. No external
   callback or continuation pointer is stored; host/runtime remain process-lived. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_dispatch_present(void* services_user,
    rsf_d3d11_present_dispatch*);
/* Same exact-inner continuation contract for the tagged native methods. Unknown
   methods are invalid. Unowned receivers/arguments are never dereferenced and
   their result is preserved. Release consumes exactly the caller's reference:
   no additional lease is taken and no object access follows a final release.
   All other methods borrow a live receiver and arguments synchronously. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_dispatch_chain(void* services_user,
    rsf_d3d11_chain_dispatch*);
typedef struct rsf_d3d11_native_chain {
    uint32_t struct_size, abi_version;
    void* device;
    void* immediate_context;
    void** swapchain;
} rsf_d3d11_native_chain;
typedef struct rsf_d3d11_creation_scope {
    uint32_t struct_size, abi_version;
    void* window;
    uint64_t token;
} rsf_d3d11_creation_scope;
/* Mode 3: bracket the adapter's full native creation call on the same thread.
   Begin requires token=0. End is mandatory after successful Begin; null result
   cancels. A nonnull result borrows final outer outputs and verifies the captured
   inner owner without replacing/releasing those outputs. Nested scopes refuse.
   All structures are size/version checked; token/window must match Begin. */
/* A valid scope with a malformed result still closes/cancels. A malformed scope
   header or wrong token/thread leaves the real owning scope open for correct End. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_creation_begin(void*, rsf_d3d11_creation_scope*);
RSF_RUNTIME_API int32_t rsf_d3d11_host_creation_end(void*, const rsf_d3d11_creation_scope*, const rsf_d3d11_native_chain*);
/* Cold creation owner, after the complete outer native creation call returns.
   Device/context are borrowed. On success replaces the caller-owned chain reference
   with the final application facade and releases that original reference. Failure
   preserves the native outputs/refcounts. No native Present is called or bypassed.
   The adapter preserves the original creation HRESULT and supplies native policy. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_wrap_native_swapchain(void* services_user,
    const rsf_d3d11_native_chain*);

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
#define RSF_D3D11_CAPTURE_CONTROL_VERSION 1u
#define RSF_D3D11_CAPTURE_SEQUENCE 1u
#define RSF_D3D11_CAPTURE_FAILURES 2u
typedef struct rsf_d3d11_capture_control {
    uint32_t struct_size, abi_version;
    uint32_t operation, value; /* sequence requires 1; failures accepts 0 or 1 */
} rsf_d3d11_capture_control;
/* Any thread. Uses the same bounded controls as the shared overlay. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_capture_control(const rsf_d3d11_capture_control*);


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
/* Restoration is permitted while suspended so quiesce can drain. On the verified
   session and graphics-owner thread, no pending transaction is idempotent OK,
   including preparation already rolled back. A pending lease still requires its
   exact native frame/view scope; mismatch never cancels that lease. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_restore_auxiliaries(void* services_user,
    const rsf_d3d11_auxiliary_scope*);

/* Independent request negotiation: existing Host/Game service layouts do not change. */
#define RSF_D3D11_REDUCED_FRAME_ABI_VERSION 1u
#define RSF_D3D11_REDUCED_RESERVE 0u
#define RSF_D3D11_REDUCED_SEAL 1u
#define RSF_D3D11_REDUCED_FINISH 2u
#define RSF_D3D11_REDUCED_RETIRE 3u
#define RSF_D3D11_REDUCED_CANCEL 4u
#define RSF_D3D11_REDUCED_NONE 0u
#define RSF_D3D11_REDUCED_RESERVED 1u
#define RSF_D3D11_REDUCED_SEALED 2u
#define RSF_D3D11_REDUCED_TEMPORAL 3u
#define RSF_D3D11_REDUCED_SPATIAL 4u
#define RSF_D3D11_REDUCED_CANCELLED 5u
#define RSF_D3D11_REDUCED_UNSAFE 6u
#define RSF_D3D11_REDUCED_CLEANUP_PENDING 7u
#define RSF_D3D11_REDUCED_FULL_OUTPUT_DEFINED 0x1u
#define RSF_D3D11_REDUCED_TEMPORAL_OUTPUT 0x2u
#define RSF_D3D11_REDUCED_SPATIAL_OUTPUT 0x4u
#define RSF_D3D11_REDUCED_NEXT_REDUCTION_ALLOWED 0x8u
#define RSF_D3D11_REDUCED_CLEANUP_REQUIRED 0x10u
#define RSF_D3D11_REDUCED_REQUIRED_RECOVERY_FAILED 0x20u
#define RSF_D3D11_REDUCED_TEMPORAL_ATTEMPTED 0x40u
typedef struct rsf_d3d11_reduced_frame {
    uint32_t struct_size, abi_version;
    uint32_t operation, disposition; /* disposition is host output */
    uint64_t token; /* zero on RESERVE; host output, exact identity thereafter */
    const rsf_game_render_pass* owner;
    uint32_t surface_count, reserved; /* reserved must be zero */
    const rsf_d3d11_auxiliary_surface* surfaces;
    int32_t temporal_result; /* host output, meaningful with TEMPORAL_ATTEMPTED */
    uint32_t result_flags; /* host output */
} rsf_d3d11_reduced_frame;
/* Graphics-owner transaction, independent of temporal camera admission. owner and
   surfaces are synchronous borrowed facts; retained descriptors and resource leases
   are copied. RESERVE allocates without reading/writing native contents or bindings,
   before native reduction. SEAL snapshots the exact produced source. FINISH retains
   actual matching temporal success or reconstructs numeric-nearest colour and raw
   consumers, without fabricating temporal inputs/success. Initial spatial mapping
   requires integer source rectangles and zero resampling offsets for all surfaces.
   RETIRE drains content obligations on the same owner, including while suspended.
   CANCEL is valid only for an unused reservation. Failures retain pending ownership;
   a successful Main is never rolled back by later retirement failure. No next
   reduction is authorized until the prior ticket drains. Unknown/device-lost owners
   refuse. All output fields come from the host, never caller success claims. */
RSF_RUNTIME_API int32_t rsf_d3d11_host_reduced_frame(void* services_user, void* immediate_context,
    rsf_d3d11_reduced_frame*);

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
