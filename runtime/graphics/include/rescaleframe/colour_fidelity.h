/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_COLOUR_FIDELITY_H
#define RSF_COLOUR_FIDELITY_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_colour_fidelity rsf_colour_fidelity;
/* Correct RGB drift against the current scene's HDR colour. Depth discontinuities retain
   reconstruction; source bounds prevent added peaks/ringing. Alpha passes through.
   Jitter is the producer's screen displacement in render pixels, with Y down. No history. */
int rsf_colour_fidelity_create(void* device, rsf_colour_fidelity** out);
int rsf_colour_fidelity_run(rsf_colour_fidelity* pass, void* immediate_context,
                            void* scene_colour, void* reconstructed_colour, void* depth,
                            uint32_t depth_inverted, const float jitter[2]);
void rsf_colour_fidelity_destroy(rsf_colour_fidelity* pass);
#ifdef __cplusplus
}
#endif
#endif
