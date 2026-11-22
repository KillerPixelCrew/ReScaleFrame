// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr.h>

/* An ordered receipt means native writes are ordered on the immediate context.
   The host still checks device life before retiring resources. */
#ifdef __cplusplus
extern "C"
#endif
int32_t rsf_native_sr_evaluate_ordered_guarded(void* context,
    const rsf_game_render_pass* pass, uint32_t* ordered)
#ifdef __cplusplus
    noexcept
#endif
    ;
