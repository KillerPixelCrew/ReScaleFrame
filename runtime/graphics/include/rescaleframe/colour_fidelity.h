/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_COLOUR_FIDELITY_H
#define RSF_COLOUR_FIDELITY_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_colour_fidelity rsf_colour_fidelity;
/* Correct RGB drift against this frame's HDR scene, modifying reconstructed_colour in place.
   Depth discontinuities suppress correction; source bounds prevent new peaks/ringing; alpha is
   preserved. No temporal history. Jitter is screen displacement in render pixels, Y down.
   The pass retains ID3D11Device and owns lazily resized scratch textures. Create/run return 1 on
   success and 0 on invalid input, shader/resource failure, or failed state save. Run requires the
   owning immediate context, single-sample same-device inputs, matching scene/depth extents, and
   an RGBA16F/RGBA32F output 1..8 times the scene size per axis. State is restored after dispatch. */
int rsf_colour_fidelity_create(void* device, rsf_colour_fidelity** out);
int rsf_colour_fidelity_run(rsf_colour_fidelity* pass, void* immediate_context,
                            void* scene_colour, void* reconstructed_colour, void* depth,
                            uint32_t depth_inverted, const float jitter[2]);
void rsf_colour_fidelity_destroy(rsf_colour_fidelity* pass);
#ifdef __cplusplus
}
#endif
#endif
