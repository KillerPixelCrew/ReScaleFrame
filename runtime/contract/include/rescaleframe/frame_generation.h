/* SPDX-License-Identifier: GPL-3.0-only */
/* Vendor-neutral frame-generation provider contract. The caller owns the presenting thread and
   resource leases; providers expose capabilities, lifecycle operations, markers, and retirement. */
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
/* Disable interpolation. */
#define RSF_FG_OFF 0u
/* Use the requested fixed generated-frame count. */
#define RSF_FG_FIXED 1u
/* Let the provider choose its supported count. */
#define RSF_FG_AUTO 2u
/* Adjust generated-frame count to meet a target frame rate. */
#define RSF_FG_DYNAMIC 3u
typedef uint32_t rsf_pacing_owner;
/* No provider owns frame pacing. */
#define RSF_PACING_NONE 0u
/* NVIDIA Reflex owns frame pacing. */
#define RSF_PACING_REFLEX 1u
/* Intel XeLL owns frame pacing. */
#define RSF_PACING_XELL 2u
typedef uint32_t rsf_reflex_mode;
#define RSF_REFLEX_OFF 0u
#define RSF_REFLEX_ON 1u
#define RSF_REFLEX_BOOST 2u
/* Bits in rsf_fg_status.valid_statistics. */
#define RSF_FG_STAT_TOTAL_PRESENTED 1u
#define RSF_FG_STAT_GENERATED_PRESENTED 2u
#define RSF_FG_STAT_LATENCY_AVAILABLE 4u
#define RSF_FG_STAT_PRESENT_CALLBACKS 8u
#define RSF_FG_STAT_ACTIVITY 16u

/* Requested frame-generation and pacing policy. */
typedef struct rsf_fg_options {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Requested interpolation policy. */
    rsf_fg_mode mode;
    /* Fixed generated frames per rendered frame. Used when mode is RSF_FG_FIXED. */
    uint32_t generated_frames;
    /* Target output rate for dynamic mode, in frames per second. */
    float dynamic_target_fps;
    /* Requested Reflex state when the provider supports Reflex. */
    rsf_reflex_mode reflex_mode;
    /* Minimum interval between rendered frames, in microseconds; zero disables the limit. */
    uint32_t frame_limit_us;
} rsf_fg_options;

/* Effective provider state and counters. Statistics are valid only when their flag is set. */
typedef struct rsf_fg_status {
    uint32_t struct_size;
    /* Non-zero if this device/runtime combination supports the provider. */
    uint32_t supported;
    /* Non-zero when generation is enabled and observed active. */
    uint32_t active;
    /* Maximum generated frames reserved by the provider. */
    uint32_t max_generated_frames;
    /* Capability flags for changing the generated count and presenting with VSync. */
    uint32_t dynamic_supported;
    uint32_t vsync_supported;
    /* Minimum supported output width or height, in pixels; zero means unspecified. */
    uint32_t min_dimension;
    /* Inserted frames requested by the effective mode; zero also covers adaptive selection. */
    uint32_t effective_generated_frames;
    rsf_fg_mode effective_mode;
    /* Provider that currently owns pacing, independent of the requested mode. */
    rsf_pacing_owner pacing_owner;
    rsf_reflex_mode effective_reflex;
    uint32_t low_latency_available;
    /* RSF_FG_STAT_* flags identify counters valid in this status record. */
    uint32_t valid_statistics;
    /* Source and generated frames reported as presented by the SDK. */
    uint64_t total_presented;
    uint64_t generated_presented;
    /* Version selected by the provider. */
    uint64_t version_id;
    /* Raw vendor status/result code, interpreted using that provider's SDK. */
    int32_t vendor_status;
    char version_name[128];
    uint32_t auto_supported;
    /* Last accepted caller policy, which can differ from effective_mode for a skipped frame. */
    rsf_fg_mode configured_mode;
    /* FFX image-finalization callbacks; these are not confirmed Present counts. */
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

/* Device, chain, runtime, and engine information used to create a provider session. */
typedef struct rsf_generation_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_fg_swapchain_desc chain;
    /* Absolute path to provider runtime files. Borrowed during create. */
    const char* runtime_directory_utf8;
    /* Provider-specific feature major and runtime version selection. */
    uint32_t feature_major;
    uint64_t version_id;
    /* Optional Streamline-only assertions against the previously created host. Null identity
       strings and zero optional fields inherit the host; they never initialize another SDK. */
    uint32_t engine_type;
    const char* engine_version_utf8;
    const char* project_id_utf8;
    uint32_t require_signature;
    uint32_t development_runtime;
    /* Fixed input conventions for this context; changing them may require recreation. */
    uint32_t depth_inverted;
    uint32_t depth_infinite;
    uint32_t motion_jittered;
    uint32_t motion_at_display_resolution;
    uint32_t hdr;
    /* Positive multiplier converting view-space distances to meters. */
    float view_space_to_meters;
    /* Borrowed rsf_streamline_host created before graphics activation. Required for DLSS-G. */
    void* streamline_host;
} rsf_generation_setup;

/** Provider function table. Except for marker(), call on the presentation-owner thread. The device,
   queue, and window must outlive the session. Frame metadata is borrowed during prepare; resource
   leases remain the caller's responsibility through GPU completion and provider retirement.
   Typical ordering is create/configure, begin_frame, CPU/render markers, prepare, Present,
   after_present, then status/retirement. begin_frame may perform vendor latency sleep.
   One source identity must follow input through all markers and Present; abandon it with abort_frame
   when that path cannot complete. status is a cached snapshot and does not perform Present.
   Destruction follows GPU drain and retirement, with all concurrent marker callers stopped. */
typedef struct rsf_generation_provider {
    uint32_t struct_size;
    /* Create an opaque session and borrowed IDXGISwapChain. Null hwnd permits capability-only
       creation for providers supporting it; configure/prepare then require a real chain.
       Initialize setup size/version. Destroy the session using this same table. */
    rsf_backend_result (*create)(const rsf_generation_setup*, void** session, void** present_chain);
    /* Apply options to the created session. */
    rsf_backend_result (*configure)(void* session, const rsf_fg_options* options);
    /* Begin bookkeeping for one rendered frame before submitting its latency markers. */
    rsf_backend_result (*begin_frame)(void* session, uint64_t frame_id);
    /* Submit a vendor-neutral latency marker for the specified frame. */
    rsf_backend_result (*marker)(void* session, rsf_latency_marker marker, uint64_t frame_id,
                                 uint32_t controller_input);
    /* Tag the frame's resources and record generation work on command_list. */
    rsf_backend_result (*prepare)(void* session, void* command_list, const rsf_fg_frame* frame);
    /* Complete provider work that is defined to follow Present. */
    rsf_backend_result (*after_present)(void* session);
    /* Query effective options and counters. Initialize status.struct_size before calling;
       only counters indicated by valid_statistics are meaningful. */
    rsf_backend_result (*status)(void* session, rsf_fg_status* status);
    /* Initialize retirement.struct_size. Return a borrowed fence/value for the last vendor read;
       a null fence does not remove the caller's submission-fence obligation. */
    rsf_backend_result (*retirement)(void* session, rsf_fg_retirement* retirement);
    /* Destroy the session after all provider reads have retired. */
    void (*destroy)(void* session);
    /* Abandon a frame whose presentation path will not complete. */
    rsf_backend_result (*abort_frame)(void* session, uint64_t frame_id);
} rsf_generation_provider;

/** Return immutable module-owned tables. Builds without SDK headers retain the tables and return
 * NOT_COMPILED from create and feature operations; this interface has no separate probe callback.
 */
const rsf_generation_provider* rsf_generation_dlss(void);
const rsf_generation_provider* rsf_generation_fsr(void);
const rsf_generation_provider* rsf_generation_xess(void);
#ifdef __cplusplus
}
#endif
#endif
