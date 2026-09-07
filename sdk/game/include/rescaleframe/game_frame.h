/* SPDX-License-Identifier: MIT */
/* What a frame is, as the plugin and the runtime jointly know it.
 *
 * MIT like the rest of the game SDK, and self-contained for the same reason: a plugin compiles
 * against this without inheriting the GPL from the rest of the repository, so it includes nothing
 * but stdint.
 *
 * There is one record per frame and three parties write to it, which is the whole reason it exists
 * rather than each party keeping its own idea of what frame this is:
 *
 *   the plugin       assigns the identifier at the input boundary, names the screen, fills the
 *                    camera. It is the only one that can, because only it knows the engine.
 *   the orchestrator stamps the resource generation and the frame time.
 *   the presentation stamps the present index.
 *
 * Frame generation is what makes this necessary. Every vendor wants to know which frame a resource
 * belongs to, and a resource tagged with the wrong frame produces a plausible, wrong picture rather
 * than an error: the interpolation is between two moments that were never adjacent. Guessing from a
 * counter that increments somewhere in the renderer is exactly the mistake Unreal's own comments
 * warn against, which is why the identifier comes from the input boundary and travels.
 */

#ifndef RSF_GAME_FRAME_H
#define RSF_GAME_FRAME_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_GAME_FRAME_ABI_VERSION 1u

/* Monotonic, plugin-assigned, never reused within a session. Zero means no frame, which is what a
   caller sends when it genuinely does not know rather than guessing at one. */
typedef uint64_t rsf_frame_id;
#define RSF_FRAME_ID_NONE ((rsf_frame_id)0)

/* How far through the pipeline a frame has got. A phase is reported when it happens rather than
   inferred, because the interesting failures are frames that skip one. */
typedef uint32_t rsf_frame_phase;
#define RSF_PHASE_INPUT ((rsf_frame_phase)0)
#define RSF_PHASE_SIMULATION ((rsf_frame_phase)1)
#define RSF_PHASE_RENDER_SUBMIT ((rsf_frame_phase)2)
#define RSF_PHASE_SR_EVALUATED ((rsf_frame_phase)3)
#define RSF_PHASE_HUDLESS_CAPTURED ((rsf_frame_phase)4)
#define RSF_PHASE_UI_COMPLETE ((rsf_frame_phase)5)
#define RSF_PHASE_PRESENTED ((rsf_frame_phase)6)

/* What the player is looking at. This decides what is allowed to run: interpolating a menu produces
   a smeared menu, and reconstructing a video produces a soft one.
 *
 * UNKNOWN is not a failure and not a default to act on. A policy that treats unknown as flight will
 * eventually interpolate a cutscene, so the safe reading of unknown is the conservative one. */
typedef uint32_t rsf_screen_class;
#define RSF_SCREEN_UNKNOWN ((rsf_screen_class)0)
#define RSF_SCREEN_MENU ((rsf_screen_class)1)
#define RSF_SCREEN_BRIEFING ((rsf_screen_class)2)
#define RSF_SCREEN_HANGAR ((rsf_screen_class)3)
#define RSF_SCREEN_FLIGHT ((rsf_screen_class)4)
#define RSF_SCREEN_REPLAY ((rsf_screen_class)5)
#define RSF_SCREEN_VIDEO ((rsf_screen_class)6)
#define RSF_SCREEN_LOADING ((rsf_screen_class)7)

/* Latency markers, numbered as XeLL numbers them so the common case is a pass-through. Streamline's
   PCL uses different values and the backend maps them; putting the mapping there rather than here
   keeps a vendor's numbering out of a plugin's sight. */
typedef uint32_t rsf_latency_marker;
#define RSF_LATENCY_SIMULATION_START ((rsf_latency_marker)0)
#define RSF_LATENCY_SIMULATION_END ((rsf_latency_marker)1)
#define RSF_LATENCY_RENDER_SUBMIT_START ((rsf_latency_marker)2)
#define RSF_LATENCY_RENDER_SUBMIT_END ((rsf_latency_marker)3)
#define RSF_LATENCY_PRESENT_START ((rsf_latency_marker)4)
#define RSF_LATENCY_PRESENT_END ((rsf_latency_marker)5)
#define RSF_LATENCY_INPUT_SAMPLE ((rsf_latency_marker)6)

/* The history is not usable: a cut, a teleport, a resize, the first frame. Every reconstruction and
   every interpolator wants this, and the cost of missing one is a smear that lasts until the
   history recovers. */
#define RSF_FRAME_FLAG_RESET 0x1u
/* Do not reconstruct this frame. */
#define RSF_FRAME_FLAG_NO_SR 0x2u
/* Do not generate frames around this one. */
#define RSF_FRAME_FLAG_NO_FG 0x4u
/* The identifier was not carried from the input boundary and was taken from the most recently begun
   frame instead. Reported rather than hidden: a vendor that interpolates on frame identity has to
   be told to stop, and one that interpolates on present index need not be. */
#define RSF_FRAME_FLAG_AMBIGUOUS_ID 0x8u
/* The interface was diverted out of the scene this frame, so the scene is HUD-less by construction
   rather than by a copy taken at the right moment. */
#define RSF_FRAME_FLAG_UI_DIVERTED 0x10u

/* One frame, as the plugin sees it. Engine terms, no vendor terms.
 *
 * Moved here from the orchestrator, where it lived while its shape settled. A plugin fills it, the
 * orchestrator converts it to whatever a vendor asks for, and neither knows the other's language.
 * Every field is a decision that has been got wrong at least once somewhere in this project. */
typedef struct rsf_camera_frame {
    uint32_t struct_size;
    uint32_t abi_version;

    /* Row major, and without the temporal jitter. A projection that still carries it makes the
       reconstruction correct for a camera that was never rendered, which looks like softness rather
       than like a bug. */
    float view_to_clip[16];
    float clip_to_view[16];
    float view_to_world[16];
    float world_to_view[16];
    /* This frame's clip space to the previous frame's, which is what an interpolator reprojects
       with when it has no motion vectors for a pixel. */
    float clip_to_previous_clip[16];

    /* Jitter in pixels of the render extent, with the engine's own sign convention already applied.
       Not in clip space and not normalised: every vendor asks for pixels, and converting once here
       is one place to be wrong instead of three. */
    float jitter_pixels[2];
    float previous_jitter_pixels[2];

    /* The view rectangle, which is not the buffer. Unreal renders into a sub-rectangle of pooled
       targets, so at a reduced scale the two differ and using the buffer scales every offset by the
       render scale without ever looking wrong. */
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* Near and far as the projection has them, so a backend that wants them need not re-derive them
       from a matrix it was also given. */
    float near_plane;
    float far_plane;
    float vertical_fov_radians;

    /* Non-zero when the depth buffer is reversed, which Unreal's is. */
    uint32_t depth_inverted;
    /* Non-zero when motion vectors still carry the jitter. */
    uint32_t motion_jittered;
    /* Seconds since the previous frame, as the engine measured it rather than as a wall clock did. */
    float frame_time_seconds;
} rsf_camera_frame;

/* The record itself. Appended to, never reordered, like everything else in this SDK. */
typedef struct rsf_frame_record {
    uint32_t struct_size;
    uint32_t abi_version;

    rsf_frame_id frame_id;
    /* Distinguishes a frame in this session from one in a session before a restart, so a stale
       resource cannot be mistaken for a current one after the pipeline is rebuilt. */
    uint64_t session_id;
    /* Which view of the frame this is. A frame renders several and only one is the player's; the
       others produce plausible, wrong pictures if reconstructed. */
    uint32_t view_id;
    /* Bumped whenever the surfaces are recreated: a resize, a render size change, a swap chain
       rebuild. A resource carrying an older generation is stale and must not be tagged. */
    uint32_t resource_generation;

    rsf_frame_phase phase;
    uint32_t flags;
    rsf_screen_class screen;

    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* When input for this frame was sampled, from the platform's high resolution counter. This is
       what makes a latency number mean anything: everything else measures a part of the pipeline,
       and this measures the part the player feels. */
    uint64_t input_qpc;
    float frame_time_ms;

    /* Contiguous over non-test presents, assigned by the presentation side. FidelityFX and XeFG
       interpolate on this rather than on `frame_id`, which is why a frame with an ambiguous
       identifier can still be generated around. */
    uint64_t present_index;

    rsf_camera_frame camera;
} rsf_frame_record;

/* Whether a frame may be reconstructed or generated around, given its record.
 *
 * A function rather than a rule each caller repeats: this is asked in at least four places and the
 * answer has to be the same in all of them, or the counters disagree with the picture. */
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

static inline int rsf_frame_allows_fg(const rsf_frame_record* record)
{
    if (!rsf_frame_allows_sr(record)) {
        return 0;
    }
    if ((record->flags & RSF_FRAME_FLAG_NO_FG) != 0) {
        return 0;
    }
    /* A menu holds still and then jumps, which is the worst case for an interpolator: there is no
       motion to interpolate and then a discontinuity to smear. Unknown is refused for the same
       reason it is never treated as flight. */
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
