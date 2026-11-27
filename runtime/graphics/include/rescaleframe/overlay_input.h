/* SPDX-License-Identifier: MIT */
/* Window input for the shared overlay. The Rust host subclasses the presentation window and
   passes input through while the panel is hidden. While it is open, the panel consumes supported
   pointer and keyboard messages. Releases for presses already seen by the game still reach the
   game, as do system keys other than the configured toggle. Input sent directly to game-specific
   device APIs may bypass the window. Cursor capture uses User32 detours when available. */

#ifndef RSF_OVERLAY_INPUT_H
#define RSF_OVERLAY_INPUT_H

#include <rescaleframe/overlay.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OVERLAY_INPUT_ABI_VERSION 1u

typedef int32_t rsf_overlay_input_result;
#define RSF_OVERLAY_INPUT_OK ((rsf_overlay_input_result)0)
#define RSF_OVERLAY_INPUT_ERROR_INVALID_ARGUMENT ((rsf_overlay_input_result)-1)
#define RSF_OVERLAY_INPUT_ERROR_ABI_MISMATCH ((rsf_overlay_input_result)-2)
#define RSF_OVERLAY_INPUT_ERROR_ALREADY_INSTALLED ((rsf_overlay_input_result)-3)
#define RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED ((rsf_overlay_input_result)-4)
#define RSF_OVERLAY_INPUT_ERROR_SUBCLASS_FAILED ((rsf_overlay_input_result)-5)
/* Uninstall found a window procedure that is not ours, so something subclassed the window after we
   did. Restoring our saved pointer would remove that tool's hook, so nothing is restored and the
   overlay stays installed. Unloading this module in that state ends the process the next time a
   message arrives. */
#define RSF_OVERLAY_INPUT_ERROR_FOREIGN_SUBCLASS ((rsf_overlay_input_result)-6)

/* Same shape and same reason as the other sinks in this directory: a step is announced before it is
   taken, because a returned code from inside a game process often never arrives. */
typedef void (*rsf_overlay_input_log_fn)(void* user, const char* message);

typedef struct rsf_overlay_input_options {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Virtual key that opens and closes the overlay. Zero means `VK_INSERT` (0x2D).
       A caller may choose another key for a different game. */
    uint32_t toggle_virtual_key;
    rsf_overlay_input_log_fn log;
    void* log_user;
} rsf_overlay_input_options;

/* Subclass the game's window.

   `window` is the `HWND` the swap chain was created against, passed in rather than searched for:
   this module has no way to tell the game's main window from a splash screen or a hidden message
   window, and the swap chain owner already knows. `IDXGISwapChain::GetDesc` reports it.

   Safe to call from any thread. It touches no graphics state.

   The window procedure is replaced with the wide variant. On a window registered through
   `RegisterClassA` that converts the window to Unicode, which changes how the game's own procedure
   receives `WM_CHAR`. The install logs when it sees such a window; the fix, if a game turns up that
   needs it, is the `A` variants of both the subclass and the forward. */
rsf_overlay_input_result rsf_overlay_input_install(void* window,
                                                   const rsf_overlay_input_options* options);

/* Restore the original window procedure.

   Refuses with `RSF_OVERLAY_INPUT_ERROR_FOREIGN_SUBCLASS` if the window no longer holds our
   procedure. A window the game has already destroyed leaves nothing to restore, so the state is
   dropped and this returns `RSF_OVERLAY_INPUT_OK`. Restoring cannot make a message already inside
   the hook finish first, so the same race the other hooks in this directory carry applies:
   uninstall from a point where the message thread is known to be elsewhere, or stay installed for
   the process's life.

   The window procedure is what comes back. If install converted an ANSI window to Unicode, that
   conversion stays for the life of the process. */
rsf_overlay_input_result rsf_overlay_input_uninstall(void);

/* Whether the overlay is open. Nonzero if it is. Safe from any thread, and cheap enough to call per
   frame. */
uint32_t rsf_overlay_input_visible(void);

/* Present owner, before testing visibility or skipping a hidden panel. Polls the
   configured toggle's high key-state bit while this window is foreground. This
   also handles DirectInput paths that do not deliver WM_KEYDOWN. A held key is
   shared with the message route; focus gain while held is not a new press. */
void rsf_overlay_input_poll_toggle(void);

/* Request a specific visibility state. A transition either way
   drops the accumulated buttons and wheel, so a click held while the overlay opens is not delivered
   as a click on whatever appeared under the cursor. */
void rsf_overlay_input_set_visible(uint32_t visible);

/* Fill one frame's input for `rsf_overlay_frame`.

   `out->struct_size` is set by the caller and is left alone. The wheel is accumulated between calls
   and cleared by this one, so a notch is delivered exactly once no matter how the frame rate and
   the message rate relate. Mouse position and button state are levels, not events, and survive the
   call.

   `display_width` and `display_height` are what the renderer is about to draw into. They come from
   the caller so that this module and the renderer cannot disagree about the target, and they are
   copied into the structure rather than measured here.

   Returns `RSF_OVERLAY_INPUT_ERROR_NOT_INSTALLED` when no window is subclassed, having filled the
   structure with a hidden, idle state so a caller that ignores the code still draws nothing rather
   than reading uninitialised memory. */
rsf_overlay_input_result rsf_overlay_input_collect(rsf_overlay_input* out, uint32_t display_width,
                                                   uint32_t display_height, float delta_seconds);

/* Cumulative diagnostics, counted only while the panel is open. They exist so a run can tell
   "the window never received a pointer event" from "it received and kept clicks from the game"
   when the panel cannot be captured. Pointer events are mouse moves, buttons and wheel, and
   touch/pen contacts delivered as window messages. Raw mouse reports count decoded, nonzero
   relative movement submitted to the fallback pointer accumulator. A caller diffs
   two reads to count a single open. `struct_size` is the caller's, as for every struct here. */
typedef struct rsf_overlay_input_counters {
    uint32_t struct_size;
    uint32_t installed, visible;
    /* The user32 cursor detours are installed, so the pointer position is polled each frame. */
    uint32_t cursor_freed;
    uint64_t pointer_received, pointer_consumed;
    uint64_t button_downs_consumed, wheel_consumed, raw_mouse_reports, keys_consumed;
    /* Frames sampled for the panel while open, and how many of them carried a held button. */
    uint64_t frames_visible, frames_with_button;
    /* The last pointer position sampled for the panel, in presented pixels. */
    uint32_t last_pointer_x, last_pointer_y;
} rsf_overlay_input_counters;
rsf_overlay_input_result rsf_overlay_input_counters_get(rsf_overlay_input_counters* out);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_INPUT_H */
