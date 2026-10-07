/* SPDX-License-Identifier: MIT */
/* Shared, vendor-neutral frame metadata for the game plugin and runtime.
 *
 * The plugin assigns frame identity at input sampling and supplies engine data. The orchestrator
 * records resource generation and timing; presentation records the present index. */

#ifndef RSF_GAME_FRAME_H
#define RSF_GAME_FRAME_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_GAME_FRAME_ABI_VERSION 1u

/* Monotonic within a session; zero means that no frame identity is available. */
typedef uint64_t rsf_frame_id;
#define RSF_FRAME_ID_NONE ((rsf_frame_id)0)

/* Pipeline milestone recorded for this frame. This value does not schedule work or establish
   graphics resource ownership; those contracts belong to the renderer callbacks. */
typedef uint32_t rsf_frame_phase;
#define RSF_PHASE_INPUT ((rsf_frame_phase)0)
#define RSF_PHASE_SIMULATION ((rsf_frame_phase)1)
#define RSF_PHASE_RENDER_SUBMIT ((rsf_frame_phase)2)
#define RSF_PHASE_SR_EVALUATED ((rsf_frame_phase)3)
#define RSF_PHASE_HUDLESS_CAPTURED ((rsf_frame_phase)4)
#define RSF_PHASE_UI_COMPLETE ((rsf_frame_phase)5)
#define RSF_PHASE_PRESENTED ((rsf_frame_phase)6)

/* Content class used by reconstruction and frame-generation eligibility rules. Unknown is
   intentionally distinct from flight and is handled conservatively. */
typedef uint32_t rsf_screen_class;
#define RSF_SCREEN_UNKNOWN ((rsf_screen_class)0)
#define RSF_SCREEN_MENU ((rsf_screen_class)1)
#define RSF_SCREEN_BRIEFING ((rsf_screen_class)2)
#define RSF_SCREEN_HANGAR ((rsf_screen_class)3)
#define RSF_SCREEN_FLIGHT ((rsf_screen_class)4)
#define RSF_SCREEN_REPLAY ((rsf_screen_class)5)
#define RSF_SCREEN_VIDEO ((rsf_screen_class)6)
#define RSF_SCREEN_LOADING ((rsf_screen_class)7)

/* Vendor-neutral latency events. Backends map these values to their SDK-specific markers. */
typedef uint32_t rsf_latency_marker;
#define RSF_LATENCY_SIMULATION_START ((rsf_latency_marker)0)
#define RSF_LATENCY_SIMULATION_END ((rsf_latency_marker)1)
#define RSF_LATENCY_RENDER_SUBMIT_START ((rsf_latency_marker)2)
#define RSF_LATENCY_RENDER_SUBMIT_END ((rsf_latency_marker)3)
#define RSF_LATENCY_PRESENT_START ((rsf_latency_marker)4)
#define RSF_LATENCY_PRESENT_END ((rsf_latency_marker)5)
#define RSF_LATENCY_INPUT_SAMPLE ((rsf_latency_marker)6)

/* Reset temporal history before using this frame. */
#define RSF_FRAME_FLAG_RESET 0x1u
/* Do not reconstruct this frame. */
#define RSF_FRAME_FLAG_NO_SR 0x2u
/* Do not generate frames around this one. */
#define RSF_FRAME_FLAG_NO_FG 0x4u
/* Frame identity was inferred instead of carried from the input boundary. */
#define RSF_FRAME_FLAG_AMBIGUOUS_ID 0x8u
/* UI draws were diverted from the scene, producing a HUD-less frame. */
#define RSF_FRAME_FLAG_UI_DIVERTED 0x10u

/* Camera data in engine-neutral units. Matrices are row-major. */
typedef struct rsf_camera_frame {
    /* Caller-provided structure size and ABI version. */
    uint32_t struct_size;
    uint32_t abi_version;

    /* Row-major transforms. Projection matrices exclude temporal jitter. */
    float view_to_clip[16];
    float clip_to_view[16];
    float view_to_world[16];
    float world_to_view[16];
    /* Current clip space to the previous frame's clip space. */
    float clip_to_previous_clip[16];

    /* Current and previous jitter in render-resolution pixels, using the engine's sign convention. */
    float jitter_pixels[2];
    float previous_jitter_pixels[2];

    /* View extents, which may be smaller than the backing textures. */
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* Projection near/far planes and vertical field of view. */
    float near_plane;
    float far_plane;
    float vertical_fov_radians;

    /* Non-zero when depth uses a reversed range. */
    uint32_t depth_inverted;
    /* Non-zero when motion vectors still carry the jitter. */
    uint32_t motion_jittered;
    /* Engine frame interval in seconds. */
    float frame_time_seconds;
} rsf_camera_frame;

/* Per-frame identity, eligibility, timing, dimensions, and camera state. Extend by appending fields. */
typedef struct rsf_frame_record {
    /* Caller-provided structure size and ABI version. */
    uint32_t struct_size;
    uint32_t abi_version;

    rsf_frame_id frame_id;
    /* Identifies the runtime session that owns this frame. */
    uint64_t session_id;
    /* Rendered view associated with this frame. */
    uint32_t view_id;
    /* Incremented when render or presentation resources are recreated. */
    uint32_t resource_generation;

    rsf_frame_phase phase;
    uint32_t flags;
    rsf_screen_class screen;

    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* Input-sample timestamp from the platform high-resolution counter. */
    uint64_t input_qpc;
    /* Time since the previous frame, in milliseconds. */
    float frame_time_ms;

    /* Monotonic index assigned to non-test presents. */
    uint64_t present_index;

    rsf_camera_frame camera;
} rsf_frame_record;

/* Return non-zero when the record is present, large enough, and its screen and flags allow SR.
   Policy helper only: it does not validate abi_version, camera data or resource/frame ownership. */
static inline int rsf_frame_allows_sr(const rsf_frame_record* record)
{
    if (!record || record->struct_size < sizeof(rsf_frame_record)) {
        return 0;
    }
    if ((record->flags & RSF_FRAME_FLAG_NO_SR) != 0) {
        return 0;
    }
    switch (record->screen) {
    case RSF_SCREEN_VIDEO:
    case RSF_SCREEN_LOADING:
        return 0;
    default:
        return 1;
    }
}

/* Return non-zero when SR is allowed and screen/flags permit FG. This policy helper does not
   prove that paired temporal inputs, source-frame identity or presentation ownership are valid. */
static inline int rsf_frame_allows_fg(const rsf_frame_record* record)
{
    if (!rsf_frame_allows_sr(record)) {
        return 0;
    }
    if ((record->flags & RSF_FRAME_FLAG_NO_FG) != 0) {
        return 0;
    }
    /* Menus and unknown screens are excluded because their motion or eligibility is ambiguous. */
    switch (record->screen) {
    case RSF_SCREEN_MENU:
    case RSF_SCREEN_UNKNOWN:
        return 0;
    default:
        return 1;
    }
}

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_GAME_FRAME_H */
