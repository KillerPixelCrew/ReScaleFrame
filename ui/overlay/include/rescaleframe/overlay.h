/* SPDX-License-Identifier: GPL-3.0-only */
/* GPU-independent panel and HUD ABI. Native renderers consume meshes and texture patches and
   apply settings intents. All calls on one handle require exclusive access on its owning thread.
   Set struct_size on frame input, stats, draw-data and intent structures. Append fields without
   reordering; this build requires its complete known prefix and ignores later fields in larger structures.
   Returned meshes and pixels are borrowed until the next frame call or handle destruction. */

#ifndef RSF_OVERLAY_H
#define RSF_OVERLAY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Creation requires this exact version; structure sizes are checked separately on each frame. */
#define RSF_OVERLAY_ABI_VERSION 9u
/* Bit 31 of fg_backend_choices: the presentation owner supports provider changes during play. */
#define RSF_OVERLAY_FG_RUNTIME_SWITCH 0x80000000u

typedef int32_t rsf_overlay_result;
#define RSF_OVERLAY_OK ((rsf_overlay_result)0)
#define RSF_OVERLAY_ERROR_INVALID_ARGUMENT ((rsf_overlay_result)-1)
#define RSF_OVERLAY_ERROR_ABI_MISMATCH ((rsf_overlay_result)-2)
/* An unwinding panic was caught. Destroy the poisoned handle; aborting panics cannot be caught. */
#define RSF_OVERLAY_ERROR_PANICKED ((rsf_overlay_result)-3)

/* Quality IDs shared with the Rust model and reconstruction contract. */
typedef uint32_t rsf_overlay_quality;
#define RSF_OVERLAY_QUALITY_NATIVE ((rsf_overlay_quality)0)
#define RSF_OVERLAY_QUALITY_QUALITY ((rsf_overlay_quality)1)
#define RSF_OVERLAY_QUALITY_BALANCED ((rsf_overlay_quality)2)
#define RSF_OVERLAY_QUALITY_PERFORMANCE ((rsf_overlay_quality)3)
#define RSF_OVERLAY_QUALITY_ULTRA_PERFORMANCE ((rsf_overlay_quality)4)

/* Runtime snapshot for one frame. Nonzero uint32_t flags are true. Rates derive from observed
   counters and QPC; provider requests alone do not establish activity, latency or image quality. */
typedef struct rsf_overlay_stats {
    uint32_t struct_size;

    /* Whether a backend is loaded at all, and whether the driver said yes to it. */
    uint32_t backend_loaded;
    uint32_t backend_supported;
    /* UTF-8 strings borrowed for the call. Null is allowed. The Rust scan stops at 512 bytes. */
    const char* backend_name;
    const char* refusal_reason;

    /* Physical pixel extents; zero when unknown. */
    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* Legacy session counters, distinct from the 64-bit rate inputs below. */
    uint32_t frames_presented;
    uint32_t frames_evaluated;
    uint32_t frames_refused;
    /* The backend's own last result code, so a refusal can be named rather than counted. */
    int32_t last_result;

    /* Observed input availability and decode/jitter state for this frame. */
    uint32_t have_scene_color;
    uint32_t have_depth;
    uint32_t have_motion;
    uint32_t have_exposure;
    uint32_t motion_decoded;
    uint32_t jitter_active;
    float jitter_pixels[2]; /* Render-pixel offset, x then y. */

    /* Current quality, so the interface can show what is in effect rather than what was last
       clicked. Those differ whenever a change has been requested and not yet applied. */
    rsf_overlay_quality quality;
    uint32_t enabled;
    /* Appended in ABI 2. What is actually in effect, so the panel shows the session rather than
       its own last click. `render_scale_percent` is zero when nothing has set one. */
    uint32_t debug_view_on;
    uint32_t reinsert_on;
    uint32_t reinsert_available;
    uint32_t render_scale_percent;
    uint32_t captures_written;
    /* ABI 3: gate state and verified patch availability, separate from observed jitter_active. */
    uint32_t jitter_on;
    uint32_t jitter_available;
    /* SR provider IDs: 0 Off, 1 DLSS, 2 FSR2, 3 FSR3, 4 FSR4, 5 XeSS, 6 FSR1, 7 Auto. */
    uint32_t backend;
    uint32_t requested_backend;
    int32_t last_switch_result;
    /* ABI 6: modes 0 Off, 1 Fixed, 2 Auto, 3 Dynamic; current panel requests Off/Fixed only.
       Counts exclude the rendered source frame.
       Effective settings follow suspension policy; active requires SDK-confirmed generation. */
    uint32_t fg_available, fg_requested_mode, fg_requested_generated;
    uint32_t fg_effective_mode, fg_effective_generated, fg_active, fg_max_generated;
    /* Reflex modes: 0 Off, 1 On, 2 On+Boost. Provider requirements can change the effective mode. */
    uint32_t reflex_available, reflex_requested_mode, reflex_effective_mode;
    /* Suspension reason and last provider result; see the orchestrator's fg_session.h. */
    uint32_t fg_reason;
    int32_t fg_last_result;
    uint64_t fg_total_presented; /* SDK aggregate source plus generated presents. */
    /* ABI 7: QPC ticks and ticks/second; zero clock values disable FPS estimation.
       fg_present_count_valid selects the SDK aggregate; otherwise both rates use application presents. */
    uint64_t application_presented_frames, sample_qpc, qpc_frequency;
    uint32_t show_performance_hud, fg_present_count_valid;
    /* ABI 8: requested minimum interval between rendered frames, before generation, in
       microseconds (zero is unlimited), and the refresh rate of the display showing the game in
       millihertz (zero when unknown). */
    uint32_t frame_limit_us;
    uint32_t display_refresh_mhz;
    /* ABI 9: effective provider, saved/requested provider and implemented ID bits.
       FG IDs: 0 Off, 1 DLSS-G, 3 FSR3, 4 FSR4, 5 XeSS. Bit 31 advertises runtime switching.
       A choice bit records implementation availability, not GPU compatibility. */
    uint32_t fg_backend, fg_requested_backend, fg_backend_choices;
    int32_t fg_selection_result;
} rsf_overlay_stats;

/* What the user asked for, this frame. A `*_changed` flag rather than a comparison against the
   previous value, so a caller can act once instead of every frame after a click. */
typedef struct rsf_overlay_intent {
    uint32_t struct_size;
    uint32_t quality_changed;
    rsf_overlay_quality quality;
    uint32_t enabled_changed;
    uint32_t enabled;
    /* Request a diagnostic dump of this frame's inputs. */
    uint32_t dump_requested;
    /* ABI 2: legacy diagnostic/control requests retained for layout compatibility. A request
       does not confirm execution; the host reports the result through the next stats snapshot. */
    uint32_t start_requested;
    uint32_t debug_view_changed;
    uint32_t debug_view;
    uint32_t reinsert_changed;
    uint32_t reinsert;
    /* Apply the render scale below, as a percentage. Zero percent is not a request. */
    uint32_t scale_requested;
    uint32_t scale_percent;
    uint32_t capture_requested;
    /* Appended in ABI 3. */
    uint32_t jitter_changed;
    uint32_t jitter;
    uint32_t backend_changed;
    uint32_t backend;
    /* Setting groups use the corresponding changed flag; counts exclude the source frame. */
    uint32_t fg_changed, fg_mode, fg_generated;
    uint32_t reflex_changed, reflex_mode;
    uint32_t performance_hud_changed, performance_hud;
    /* ABI 8. */
    uint32_t frame_limit_changed, frame_limit_us;
    uint32_t fg_backend_changed, fg_backend;
} rsf_overlay_intent;

/* Mouse buttons, as a bit field. */
#define RSF_OVERLAY_MOUSE_LEFT 0x1u
#define RSF_OVERLAY_MOUSE_RIGHT 0x2u
#define RSF_OVERLAY_MOUSE_MIDDLE 0x4u

typedef struct rsf_overlay_input {
    uint32_t struct_size;
    /* Physical pixels, in the presented image's coordinates. */
    float mouse_x;
    float mouse_y;
    uint32_t mouse_buttons;
    float scroll_delta; /* Vertical wheel delta in egui points. */
    uint32_t display_width;
    uint32_t display_height;
    /* Seconds since the previous frame. egui uses it for animation, and a zero is survivable. */
    float delta_seconds;
    /* Zero hides interactive settings; optional hint and HUD may still draw. State survives. */
    uint32_t visible;
    /* Appended in ABI 5. Zero hides the hint; 0..1 controls its fade. No input is captured. */
    float startup_hint_alpha;
} rsf_overlay_input;

/* One vertex, in the layout egui produces: position in physical pixels, texture coordinate, and a
   premultiplied colour as RGBA bytes in a machine word. */
typedef struct rsf_overlay_vertex {
    float x;
    float y;
    float u;
    float v;
    uint32_t color;
} rsf_overlay_vertex;

typedef struct rsf_overlay_draw_call {
    /* Index slice in draw_data. Indices are mesh-local; add vertex_offset as BaseVertexLocation. */
    uint32_t index_offset;
    uint32_t index_count;
    uint32_t vertex_offset;
    /* Scissor rectangle in physical pixels. egui relies on it, so a renderer that ignores this
       draws text spilling out of its panel. */
    uint32_t clip_x;
    uint32_t clip_y;
    uint32_t clip_width;
    uint32_t clip_height;
    /* Which texture to bind, matching an id from `rsf_overlay_texture_update`. */
    uint64_t texture_id;
} rsf_overlay_draw_call;

typedef struct rsf_overlay_draw_data {
    /* Set before the call. Arrays are borrowed; empty arrays use null with zero counts. */
    uint32_t struct_size;
    const rsf_overlay_vertex* vertices;
    uint32_t vertex_count;
    const uint32_t* indices;
    uint32_t index_count;
    const rsf_overlay_draw_call* calls;
    uint32_t call_count;
} rsf_overlay_draw_data;

/* Atlas upload or partial patch. Upload all patches before drawing the frame that emitted them. */
typedef struct rsf_overlay_texture_update {
    uint64_t id;
    /* Where the patch goes in the destination texture, and how big it is. A full update has
       offsets of zero and dimensions equal to the whole texture. */
    uint32_t x;
    uint32_t y;
    uint32_t width;
    uint32_t height;
    /* Tightly packed RGBA bytes, width * height * 4 of them. Borrowed until the next call to
       `rsf_overlay_frame`. */
    const uint8_t* pixels;
    /* Whether the destination has to be created or recreated at this size before the patch. */
    uint32_t is_whole_texture;
} rsf_overlay_texture_update;

typedef struct rsf_overlay rsf_overlay;

/* Create an owning handle. Null means version mismatch or a caught construction panic.
   Standard allocator aborts are not converted to null. */
rsf_overlay* rsf_overlay_create(uint32_t abi_version);
/* Release the handle, including a poisoned one. Null is a no-op; never destroy twice. */
void rsf_overlay_destroy(rsf_overlay* overlay);

/* Lay out one frame.

   `draw_data` and `intent` are filled in. Both may be null if the caller wants only the other. The
   arrays in `draw_data` belong to the overlay and are valid until the next frame call or destruction.
   Only consume outputs after OK. Invalid arguments do not advance the frame or clear outputs.
   Foreign pointers must be readable/writable, aligned and nonoverlapping; null/size checks do
   not validate arbitrary addresses. A panic can leave old outputs, and poisons the handle. */
rsf_overlay_result rsf_overlay_frame(rsf_overlay* overlay, const rsf_overlay_input* input,
                                     const rsf_overlay_stats* stats,
                                     rsf_overlay_draw_data* draw_data,
                                     rsf_overlay_intent* intent);

/* Collect texture atlas changes produced by the last `rsf_overlay_frame`, and the ids of textures
   the overlay has finished with.

   Writes up to the supplied capacity and drains each entry once. Repeat until zero. Zero also
   covers invalid arguments or a poisoned handle. Pixel memory remains borrowed until the next
   frame or destruction; undrained work is discarded by the next frame. */
uint32_t rsf_overlay_texture_updates(rsf_overlay* overlay, rsf_overlay_texture_update* updates,
                                     uint32_t max_updates);
/* Drain texture IDs to release after drawing this frame, never before its meshes use them. */
uint32_t rsf_overlay_textures_to_free(rsf_overlay* overlay, uint64_t* ids, uint32_t max_ids);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_H */
