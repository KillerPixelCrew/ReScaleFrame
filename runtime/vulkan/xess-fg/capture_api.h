// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include "bridge_api.h"
#include "../../../sdk/game/include/rescaleframe/game_frame.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Private opt-in capture binding. The record is copied, never retained. A raw
   SDK token is associated only when its low32 matches this exact source ID. */
RSF_XESS_BRIDGE_API int32_t RsfXessBridgeCaptureSource(rsf_xess_bridge*, const rsf_frame_record*);
RSF_XESS_BRIDGE_API uint64_t RsfXessBridgeCapturedCallbacks(const rsf_xess_bridge*);
#ifdef __cplusplus
}
#endif
