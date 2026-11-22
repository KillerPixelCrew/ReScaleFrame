/* SPDX-License-Identifier: MIT */
#pragma once
/* SR world-window twin swap, mechanics (b) of docs/research/anomaly-sr-seam.md 1.4.

   Between the prologue of CRenderTarget::phase_scene_prepare (H1) and the S-B seam (first of phase_sunshafts /
   phase_blur, H2), engine render targets point at RSF-owned twins. The exchange is a plain field swap: no reference
   count changes, no SRV is created per frame.

   Stage 1 (section 9): generic0 and generic_temp only, FP16, at device size. At the seam the host runs the f^-1/f
   round trip into the engine's UNORM8 generic0.
   Stage 2 (section 10): every world target in sr_world_targets at render size, plus Device.dwWidth/dwHeight, HW.pBaseZB
   (a render-size R24G8 depth twin), HW.pBaseRT (a render-size scratch view: phase_occq binds it with HW.pBaseZB, and
   D3D11 rejects an output-size RTV with a render-size DSV) and the SSAO helper's static viewport. After H1 the
   viewport is set to render size.
   At the seam everything is restored, then the host upscales generic0 (bilinear in linear space), and nearest-copies
   rt_Position, generic1 and depth into the engine's output-size originals, and the viewport returns to output size.
   Stage 3 (section 11) adds, inside the same window: Device.mProject/mFullTransform jittered and re-uploaded through
   R_xforms::set_P, the six first-person projections jittered once each from the rmNear slot, and the sampler mip
   bias lowered through SetMipLODBias. Every exit path restores them with the twins.

   Every other exit (end of phase_combine without a seam, a stale window at the next prepare, end of frame, shutdown,
   or a failed write) also restores. sr_swap.rs owns transaction and recovery ordering; the records, skip and restore
   buckets and twin results are in sr_swap_platform.h. */
#include "sr_swap_platform.h"

/* Order of sr_world_targets (sr_profile.rs WORLD_TARGETS). Stage 1 uses the first RSF_ANOMALY_SWAP_STAGE1_SLOT_COUNT slots. */
#define RSF_ANOMALY_SWAP_SLOT_GENERIC0 0u
#define RSF_ANOMALY_SWAP_SLOT_GENERIC_TEMP 1u
#define RSF_ANOMALY_SWAP_SLOT_POSITION 2u
#define RSF_ANOMALY_SWAP_SLOT_ALBEDO 3u
#define RSF_ANOMALY_SWAP_SLOT_ACCUMULATOR 4u
#define RSF_ANOMALY_SWAP_SLOT_GENERIC1 5u
#define RSF_ANOMALY_SWAP_SLOT_GENERIC2 6u
#define RSF_ANOMALY_SWAP_SLOT_HALF_DEPTH 7u
#define RSF_ANOMALY_SWAP_SLOT_SSAO_TEMP 8u

#ifdef __cplusplus
static_assert(RSF_ANOMALY_SWAP_SLOT_SSAO_TEMP + 1 == RSF_ANOMALY_SWAP_SLOT_COUNT, "world slot order");
extern "C" {
#else
_Static_assert(RSF_ANOMALY_SWAP_SLOT_SSAO_TEMP + 1 == RSF_ANOMALY_SWAP_SLOT_COUNT, "world slot order");
#endif

/* The verified engine layout shared by both executable profiles (sr_profile.rs), for the lifetime of the process. */
const rsf_anomaly_swap_layout* rsf_anomaly_sr_engine_layout(void);

#ifdef __cplusplus
}
#endif
