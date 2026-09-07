/* SPDX-License-Identifier: GPL-3.0-only */
/* The in-game overlay: what it draws, what it was told, and what the user asked for.

   The split follows the same line as the rest of the project. egui owns the interface itself, in
   Rust, where widget state and layout are pleasant to write and testable without a GPU. D3D11 owns
   the drawing, in C++, inside the game's own frame. Between them is this header: triangles, a
   texture atlas, and the two small structures that carry state in and intent out.

   That boundary is not just taste. An overlay drawn inside somebody else's frame has to leave the
   pipeline exactly as it found it, and that is C++ work against a device the game owns. Deciding
   what a quality dropdown does is not.

   Nothing here allocates for the caller. The vertex, index and draw call arrays returned by
   `rsf_overlay_frame` are owned by the overlay and stay valid until the next call to it, which is
   the only lifetime a per frame interface needs. */

#ifndef RSF_OVERLAY_H
#define RSF_OVERLAY_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* 2: the panel drives the session rather than reporting on it, so intent gained start, debug view,
   reinsert, render scale and capture, and stats gained what is actually in effect. Both structs
   were extended by appending, which is the only way they are allowed to change. */
#define RSF_OVERLAY_ABI_VERSION 2u

typedef int32_t rsf_overlay_result;
#define RSF_OVERLAY_OK ((rsf_overlay_result)0)
#define RSF_OVERLAY_ERROR_INVALID_ARGUMENT ((rsf_overlay_result)-1)
#define RSF_OVERLAY_ERROR_ABI_MISMATCH ((rsf_overlay_result)-2)
/* The Rust side panicked and was caught at the boundary. The overlay is left in a state where it
   can be destroyed, and nothing else should be called on it. A panic must never unwind into a
   game's render thread, so it becomes this instead. */
#define RSF_OVERLAY_ERROR_PANICKED ((rsf_overlay_result)-3)

/* Quality levels, in the same order as the Rust model's `Quality` and the DLSS backend's
   `rsf_dlss_quality`, because a value that means different things in three places is a bug waiting
   for someone to add a level. */
typedef uint32_t rsf_overlay_quality;
#define RSF_OVERLAY_QUALITY_NATIVE ((rsf_overlay_quality)0)
#define RSF_OVERLAY_QUALITY_QUALITY ((rsf_overlay_quality)1)
#define RSF_OVERLAY_QUALITY_BALANCED ((rsf_overlay_quality)2)
#define RSF_OVERLAY_QUALITY_PERFORMANCE ((rsf_overlay_quality)3)
#define RSF_OVERLAY_QUALITY_ULTRA_PERFORMANCE ((rsf_overlay_quality)4)

/* What the overlay is told about the session, once per frame.

   Every field here is something the project can actually answer. There is deliberately no frames
   per second gain, no latency figure and no quality score: a presentation counter is not a latency
   measurement, and this project does not get to imply otherwise on its own status panel. */
typedef struct rsf_overlay_stats {
    uint32_t struct_size;

    /* Whether a backend is loaded at all, and whether the driver said yes to it. */
    uint32_t backend_loaded;
    uint32_t backend_supported;
    /* Vendor name and the reason a backend is unusable, when there is one. Borrowed for the
       duration of the call. Null is allowed and means "nothing to say". */
    const char* backend_name;
    const char* refusal_reason;

    uint32_t render_width;
    uint32_t render_height;
    uint32_t output_width;
    uint32_t output_height;

    /* Counters rather than rates. The overlay turns them into rates itself if it wants to, and a
       counter cannot be wrong in a way a rate can. */
    uint32_t frames_presented;
    uint32_t frames_evaluated;
    uint32_t frames_refused;
    /* The backend's own last result code, so a refusal can be named rather than counted. */
    int32_t last_result;

    /* The inputs, as found this frame. These are what a support question actually comes down to,
       and each was at some point assumed rather than checked in this project. */
    uint32_t have_scene_color;
    uint32_t have_depth;
    uint32_t have_motion;
    uint32_t have_exposure;
    uint32_t motion_decoded;
    uint32_t jitter_active;
    float jitter_pixels[2];

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
} rsf_overlay_stats;

/* What the user asked for, this frame. A `*_changed` flag rather than a comparison against the
   previous value, so a caller can act once instead of every frame after a click. */
typedef struct rsf_overlay_intent {
    uint32_t struct_size;
    uint32_t quality_changed;
    rsf_overlay_quality quality;
    uint32_t enabled_changed;
    uint32_t enabled;
    /* The user asked for the current frame's inputs to be written out, which is how a picture gets
       checked rather than assumed. */
    uint32_t dump_requested;
    /* Appended in ABI 2, when the panel took over from the hotkeys.

       Each is a request made once, on the frame the user clicked, and none of them is a statement
       about what happened: the host decides, and says so through the stats and its refusal line.
       Start is separate from `enabled` because bringing a backend up and choosing to reconstruct
       are different acts, and the first can fail in ways the second cannot. */
    uint32_t start_requested;
    uint32_t debug_view_changed;
    uint32_t debug_view;
    uint32_t reinsert_changed;
    uint32_t reinsert;
    /* Apply the render scale below, as a percentage. Zero percent is not a request. */
    uint32_t scale_requested;
    uint32_t scale_percent;
    uint32_t capture_requested;
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
    float scroll_delta;
    uint32_t display_width;
    uint32_t display_height;
    /* Seconds since the previous frame. egui uses it for animation, and a zero is survivable. */
    float delta_seconds;
    /* When zero, the overlay lays out nothing and returns no draw calls. Hiding it is not the same
       as destroying it: widget state survives, so reopening it does not reset what was chosen. */
    uint32_t visible;
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
    uint32_t struct_size;
    const rsf_overlay_vertex* vertices;
    uint32_t vertex_count;
    const uint32_t* indices;
    uint32_t index_count;
    const rsf_overlay_draw_call* calls;
    uint32_t call_count;
} rsf_overlay_draw_data;

/* A change to the overlay's texture atlas. The font atlas arrives as a full update on the first
   frame and as partial ones afterwards, so a renderer that only handles full updates will look
   correct until the moment a glyph is first used. */
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

/* Create the overlay. Returns null on an ABI mismatch or an allocation failure. */
rsf_overlay* rsf_overlay_create(uint32_t abi_version);
void rsf_overlay_destroy(rsf_overlay* overlay);

/* Lay out one frame.

   `draw_data` and `intent` are filled in. Both may be null if the caller wants only the other. The
   arrays in `draw_data` belong to the overlay and are valid until the next call to this function,
   so a renderer must consume them before the next frame rather than remember them. */
rsf_overlay_result rsf_overlay_frame(rsf_overlay* overlay, const rsf_overlay_input* input,
                                     const rsf_overlay_stats* stats,
                                     rsf_overlay_draw_data* draw_data,
                                     rsf_overlay_intent* intent);

/* Collect texture atlas changes produced by the last `rsf_overlay_frame`, and the ids of textures
   the overlay has finished with.

   Writes up to `max_updates` entries and returns how many were written. Call it after every frame:
   egui grows its atlas as glyphs are first used, so updates arrive long after startup. */
uint32_t rsf_overlay_texture_updates(rsf_overlay* overlay, rsf_overlay_texture_update* updates,
                                     uint32_t max_updates);
uint32_t rsf_overlay_textures_to_free(rsf_overlay* overlay, uint64_t* ids, uint32_t max_ids);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_H */
