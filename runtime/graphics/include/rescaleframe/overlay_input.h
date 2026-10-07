/* SPDX-License-Identifier: GPL-3.0-only */
/* Capture overlay mouse input by subclassing the caller's game window. Hidden state forwards
   messages; visible state consumes gameplay input while forwarding system keys and releases for
   presses the game already saw. Mouse position is reported in presented-image pixels and wheel
   motion in notches. Keyboard/text input is consumed but has no field in the current overlay ABI.

   Optional user32 detours suppress cursor warps/hides, lift clipping, freeze the position returned
   to the game, and neutralize raw mouse records while visible. Failed cursor-hook setup falls back
   to message capture. DirectInput/XInput polling remains outside this module. The owner coordinates
   uninstall with active message/user32 calls and keeps callbacks/module code alive until quiescent. */

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

/* Synchronous diagnostic callback. Message storage is borrowed; do not reenter install,
   uninstall or visibility changes because lifecycle/cursor locks may be held. */
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

/* Consume accumulated F1..F12 press bits (bit n means F(n+1)); toggle key is excluded. sources,
   if nonnull, gets route flags for the batch: bit 16 window messages, bit 17 raw keyboard input.
   These flags are not mapped separately per key. Callers also polling key state deduplicate their
   own hotkey triggers. Safe to consume atomically from a worker thread. */
uint32_t rsf_overlay_input_take_function_keys(uint32_t* sources);

/* Request visibility, dropping accumulated buttons/wheel on a transition. Changes within 250 ms
   of the previous transition are ignored to deduplicate message/poll toggles; this includes explicit
   requests. Before install, collect still returns hidden idle state. Coordinate with lifecycle. */
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

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_INPUT_H */
