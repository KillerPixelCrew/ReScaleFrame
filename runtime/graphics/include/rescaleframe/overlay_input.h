/* SPDX-License-Identifier: GPL-3.0-only */
/* Mouse and keyboard for the overlay, taken from the game's own window.

   The research carrier polls `GetAsyncKeyState` on a worker thread, and `loader/README.md` says
   why: a polled key state cannot disturb the message loop or the order in which the game sees its
   own input. That is the right tool for a hotkey and the wrong one for an interface. Polling has no
   mouse position, no wheel, no notion of which window had focus, and above all no way to consume an
   event, so a click on a quality dropdown would also be a click in the game.

   Consuming input means being in the message path, which means replacing the window procedure with
   `SetWindowLongPtrW` and `GWLP_WNDPROC`. That is a heavier intervention than polling, so the rules
   below exist to keep it from becoming the game's problem:

   - While the overlay is hidden this passes every message through unchanged, including the toggle
     key's own press. The game sees exactly the input stream it would have seen with nothing
     installed.
   - While the overlay is visible the mouse and keyboard messages it consumes stop here. Two
     exceptions, both to keep the game from being left holding a key it can never release. A press
     the game already saw forwards its matching release. System keys, `WM_SYSKEYDOWN` and
     `WM_SYSKEYUP` other than the toggle, forward in both directions: they are window management
     rather than gameplay, and swallowing them would take Alt+F4 with them.
   - Uninstall restores the original procedure only if the window still holds ours. Another tool
     that subclassed after us owns the chain now, and writing our saved pointer back would delete
     its hook.

   What this does not do:

   - Keyboard state is swallowed but not delivered. `rsf_overlay_input` in `overlay.h` carries mouse
     state only, so there is nowhere to put a key. Adding text entry to the overlay means adding
     fields there first, and filling them here.
   - Input that never reaches the window procedure cannot be intercepted. DirectInput device polling
     and XInput read the device directly, so a gamepad still flies the aircraft while the overlay is
     open. Raw input is intercepted only in its `WM_INPUT` form.
   - Nothing here makes a cursor visible. See the note in `overlay_input.cpp`.

   None of this has been exercised in a game yet. It is written against the documented behaviour of
   the message path, not against an observed AC7 session. */

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
    /* Virtual key that opens and closes the overlay. Zero means `VK_F7` (0x76), chosen because the
       research carrier already claims F9 through F11 and AC7 binds none of them. It is a parameter
       because a key that is free in one game is bound in the next. */
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

/* Open or close the overlay from code rather than from the toggle key. A transition either way
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

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_OVERLAY_INPUT_H */
