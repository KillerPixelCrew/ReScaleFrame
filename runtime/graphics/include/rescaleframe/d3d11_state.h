/* SPDX-License-Identifier: GPL-3.0-only */
/* Take everything the device context has bound, and put it back.

   Work done inside a game's frame has to be invisible to the game. Present is the forgiving place
   for that, because the frame is over and the next one sets its own state up. Anywhere else is not:
   the engine is halfway through a pass, it caches what it has already bound, and it will not rebind
   what it believes is still there. A reconstruction that runs before the tonemapper is exactly that
   case, and a vendor runtime is under no obligation to leave the pipeline as it found it. Streamline
   says so in as many words.

   So this saves generously rather than saving what a particular caller expects to disturb. Guessing
   which stage a closed source runtime touches is how the wrong stage ends up wrong on a machine
   nobody tested on. The cost is a few hundred reference counts per call, which does not register
   next to the work being wrapped.

   Not saved, deliberately: predication, stream output targets, and the deferred context's command
   list state. Nothing in this project sets them, and a saved value that was never disturbed is
   still a chance to restore it wrongly.

   The structure is a plain buffer so a C caller can put it on the stack. Its contents are private
   and reading them is not part of the contract. */

#ifndef RSF_D3D11_STATE_H
#define RSF_D3D11_STATE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Big enough for the state described above, checked against the real layout where it is defined.
   Oversized rather than exact: growing it is a recompile of both sides, and this header is compiled
   into a loader that is built separately. */
#define RSF_D3D11_STATE_BYTES 8192

typedef struct rsf_d3d11_state {
    /* Aligned for pointers, which is what almost all of this holds. */
    uint64_t opaque[RSF_D3D11_STATE_BYTES / 8];
} rsf_d3d11_state;

/* Read the whole pipeline into `state`. `context` is an `ID3D11DeviceContext*`.

   Every interface pointer taken here carries a reference, so a save must be matched by exactly one
   restore or the game's resources outlive their pool. Returns non-zero when the state was taken;
   zero means a null argument and nothing was saved, so nothing may be restored. */
uint32_t rsf_d3d11_state_save(void* context, rsf_d3d11_state* state);

/* Put it all back and drop the references. Safe to call only on a state a save filled in.

   The order is not arbitrary: shader resources are unbound from the stages this project uses before
   the render targets go back, because a texture bound as an input and as an output at the same
   moment is unbound by the runtime with a warning nobody sees in a shipping game. */
void rsf_d3d11_state_restore(void* context, rsf_d3d11_state* state);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_D3D11_STATE_H */
