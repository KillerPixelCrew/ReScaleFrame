/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_COLOUR_TRANSPORT_H
#define RSF_COLOUR_TRANSPORT_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* An invertible bounded encoding of linear HDR scene colour for a reconstruction that handles
   display-range input better than the engine's linear range. Per channel,
   y = (x*E / (x*E + 4))^(1/2.2), with E the engine's exposure for the frame, so the encoding follows
   scene brightness. The knee of 4 exposed units was chosen on the frozen AC7 hangar repro: knees
   0.18 to 8 all remove DLSS's dark banding, and 4 keeps a half-float encoded value within about 1.5%
   up to 50 exposed units. Decode is the exact inverse. This is a
   transport, not a tone map: decoded colour returns to the engine's own linear grading.
   The finite precision of the reconstruction's output near 1 bounds the largest recoverable
   highlight; see the AC7 research note. */
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
