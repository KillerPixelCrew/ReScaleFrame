/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_COLOUR_TRANSPORT_H
#define RSF_COLOUR_TRANSPORT_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Bounded linear-HDR transport: y = (x*E / (x*E + 4))^(1/2.2), with exposure E from the current
   frame. Decode applies its inverse before engine grading. The knee of 4 and finite half-float
   highlight limit are recorded in docs/research/ac7-motion-depth-20261004.md.
   The pass retains its device and owns encoded/scratch targets. Use one immediate-context thread;
   all inputs must be live D3D11 textures on that device. Integer results are 1 for success, 0 for
   invalid arguments, compilation, allocation, view creation, or state-save failure. */
typedef struct rsf_colour_transport rsf_colour_transport;
int rsf_colour_transport_create(void* d3d11_device, rsf_colour_transport** out);
/* Encode `source` (float RGBA, origin-zero width x height) into an owned RGBA16F texture returned
   through `encoded`. `exposure` is an optional 1x1 float texture whose red channel is E; null
   means 1. The texture stays valid until the next encode at another size, or destroy. */
int rsf_colour_transport_encode(rsf_colour_transport* transport, void* d3d11_context, void* source,
                                void* exposure, uint32_t width, uint32_t height, void** encoded);
/* Decode `target` in place (float RGBA with shader-resource binding, origin-zero width x height),
   using the same exposure texture as the matching encode. A reconstruction run on the transport
   saturates its output at 1 (measured with DLSS 310.9.1), which caps recoverable highlights at about
   926 * 4 / E. Where the output nears that cap, the optional linear `scene` (the uncoded input,
   origin-zero view scene_width x scene_height, rendered with `jitter` in render pixels) supplies the
   value instead, never darker than the reconstruction. Restores graphics state. */
int rsf_colour_transport_decode(rsf_colour_transport* transport, void* d3d11_context, void* target,
                                void* exposure, uint32_t width, uint32_t height, void* scene,
                                uint32_t scene_width, uint32_t scene_height, const float jitter[2]);
void rsf_colour_transport_destroy(rsf_colour_transport* transport);
#ifdef __cplusplus
}
#endif
#endif
