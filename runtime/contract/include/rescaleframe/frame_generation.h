/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_FRAME_GENERATION_H
#define RSF_FRAME_GENERATION_H
#include <rescaleframe/backend.h>
#ifdef __cplusplus
extern "C" {
#endif
#define RSF_FG_ABI_VERSION 1u
#define RSF_FG_BACKEND_DLSS 1u
#define RSF_FG_BACKEND_FSR3 3u
#define RSF_FG_BACKEND_FSR4 4u
#define RSF_FG_BACKEND_XESS 5u
typedef uint32_t rsf_fg_mode;
#define RSF_FG_OFF 0u
#define RSF_FG_FIXED 1u
#define RSF_FG_AUTO 2u
#define RSF_FG_DYNAMIC 3u
typedef uint32_t rsf_pacing_owner;
#define RSF_PACING_NONE 0u
#define RSF_PACING_REFLEX 1u
#define RSF_PACING_XELL 2u
typedef uint32_t rsf_reflex_mode;
#define RSF_REFLEX_OFF 0u
#define RSF_REFLEX_ON 1u
#define RSF_REFLEX_BOOST 2u
#define RSF_FG_STAT_TOTAL_PRESENTED 1u
#define RSF_FG_STAT_GENERATED_PRESENTED 2u
#define RSF_FG_STAT_LATENCY_AVAILABLE 4u
#define RSF_FG_STAT_PRESENT_CALLBACKS 8u
#define RSF_FG_STAT_ACTIVITY 16u

typedef struct rsf_fg_options {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_fg_mode mode;
    uint32_t generated_frames;
    float dynamic_target_fps;
    rsf_reflex_mode reflex_mode;
    uint32_t frame_limit_us;
} rsf_fg_options;

typedef struct rsf_fg_status {
    uint32_t struct_size;
    uint32_t supported;
    uint32_t active;
    uint32_t max_generated_frames;
    uint32_t dynamic_supported;
    uint32_t vsync_supported;
    uint32_t min_dimension;
    uint32_t effective_generated_frames;
    rsf_fg_mode effective_mode;
    rsf_pacing_owner pacing_owner;
    rsf_reflex_mode effective_reflex;
    uint32_t low_latency_available;
    uint32_t valid_statistics;
    uint64_t total_presented;
    uint64_t generated_presented;
    uint64_t version_id;
    int32_t vendor_status;
    char version_name[128];
    uint32_t auto_supported;
    rsf_fg_mode configured_mode;
    /* FFX image-finalization callbacks are not confirmed/displayed Present counts. */
    uint64_t present_callbacks;
    uint64_t generated_callbacks;
} rsf_fg_status;

typedef struct rsf_fg_retirement {
    uint32_t struct_size;
    /* Borrowed ID3D12Fence, valid until provider destruction. Null with value 0 means
       no outstanding vendor reads; the caller still honors its own GPU submission fence. */
    void* fence;
    uint64_t value;
} rsf_fg_retirement;

typedef struct rsf_generation_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_fg_swapchain_desc chain;
    const char* runtime_directory_utf8;
    uint32_t feature_major;
    uint64_t version_id;
    /* Optional Streamline-only assertions against the previously created host. Null identity
       strings and zero optional fields inherit the host; they never initialize another SDK. */
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    uint32_t require_signature;
    uint32_t development_runtime;
    uint32_t depth_inverted;
    uint32_t depth_infinite;
    uint32_t motion_jittered;
    uint32_t motion_at_display_resolution;
    uint32_t hdr;
    float view_space_to_meters;
    /* Borrowed rsf_streamline_host created before graphics activation. Required for DLSS-G. */
    void* streamline_host;
} rsf_generation_setup;

/* A versioned FG surface separate from the older draft vtable. Call on the presenting owner
   thread, except marker(), which may run at actual CPU event boundaries. Device/queue/window
   outlive the handle. Inputs and record metadata remain borrowed during prepare; GPU resources
   are leased by the caller through both GPU and provider retirement. */
typedef struct rsf_generation_provider {
    uint32_t struct_size;
    rsf_backend_result (*create)(const rsf_generation_setup*, void** session, void** present_chain);
    rsf_backend_result (*configure)(void* session, const rsf_fg_options* options);
    rsf_backend_result (*begin_frame)(void* session, uint64_t frame_id);
    rsf_backend_result (*marker)(void* session, rsf_latency_marker marker, uint64_t frame_id,
                                 uint32_t controller_input);
    rsf_backend_result (*prepare)(void* session, void* command_list, const rsf_fg_frame* frame);
    rsf_backend_result (*after_present)(void* session);
    rsf_backend_result (*status)(void* session, rsf_fg_status* status);
    rsf_backend_result (*retirement)(void* session, rsf_fg_retirement* retirement);
    void (*destroy)(void* session);
    rsf_backend_result (*abort_frame)(void* session, uint64_t frame_id);
} rsf_generation_provider;

const rsf_generation_provider* rsf_generation_dlss(void);
const rsf_generation_provider* rsf_generation_fsr(void);
const rsf_generation_provider* rsf_generation_xess(void);
#ifdef __cplusplus
}
#endif
#endif
