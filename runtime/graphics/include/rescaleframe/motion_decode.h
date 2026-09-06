/* SPDX-License-Identifier: GPL-3.0-only */
/* Turn a game's stored motion vectors into what a reconstruction backend can read.

   Every backend takes a scale factor for motion and offers nothing to subtract a bias with.
   Unreal's storage is biased, `In * (0.499 * 0.5) + 32767/65535`, so a backend handed the raw
   target reads a large constant motion across a still image. That makes this pass mandatory for
   DLSS, XeSS and FSR alike, rather than something only the backends that cannot reconstruct camera
   motion need.

   The pass is deliberately parameterised rather than written around Unreal. The encoding is a
   property of the game, so it arrives as numbers from the plugin, and the same pass serves a game
   that stores motion some other way. What lives here is the graphics work: a target, a compute
   shader, and the state save and restore around a dispatch inside somebody else's frame.

   The clear value is the part that cannot survive the decode. Unreal reserves a stored zero to mean
   "nothing wrote this pixel", which works because the bias keeps real motion away from zero. Decode
   that and zero becomes an ordinary value that real motion can take, so the sentinel has to be
   re-established explicitly at a value outside any real motion, and the backend told about it. */

#ifndef RSF_MOTION_DECODE_H
#define RSF_MOTION_DECODE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_MOTION_DECODE_ABI_VERSION 1u

typedef int32_t rsf_motion_decode_result;
#define RSF_MOTION_DECODE_OK ((rsf_motion_decode_result)0)
#define RSF_MOTION_DECODE_ERROR_INVALID_ARGUMENT ((rsf_motion_decode_result)-1)
#define RSF_MOTION_DECODE_ERROR_ABI_MISMATCH ((rsf_motion_decode_result)-2)
/* No HLSL compiler could be loaded, or the shader did not compile. */
#define RSF_MOTION_DECODE_ERROR_SHADER_FAILED ((rsf_motion_decode_result)-3)
/* A device resource could not be created. */
#define RSF_MOTION_DECODE_ERROR_RESOURCE_FAILED ((rsf_motion_decode_result)-4)
/* The source texture does not match what this pass was built for. */
#define RSF_MOTION_DECODE_ERROR_SOURCE_MISMATCH ((rsf_motion_decode_result)-5)

/* Progress and diagnostics, same shape and same reason as the texture dump sink: this runs inside
   a game's render thread, where a returned code often never arrives. */
typedef void (*rsf_motion_decode_log_fn)(void* user, const char* message);

typedef struct rsf_motion_decode_setup {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Render resolution. The source and the decoded target are both this size. */
    uint32_t width;
    uint32_t height;
    /* DXGI_FORMAT for the decoded target. Zero means DXGI_FORMAT_R16G16_FLOAT, which is what
       backends expect and what the sentinel below was chosen to fit. */
    uint32_t output_format;
    rsf_motion_decode_log_fn log;
    void* log_user;
} rsf_motion_decode_setup;

typedef struct rsf_motion_decode_params {
    uint32_t struct_size;
    /* Applied as `(stored - bias) * scale`, per axis, matching the game's storage. For Unreal:
       bias 32767/65535 on both axes and scale 1/(0.499 * 0.5). */
    float scale_x;
    float scale_y;
    float bias_x;
    float bias_y;
    /* Applied after the decode, to reach the backend's convention. Streamline wants motion in the
       [-1,1] range Unreal already decodes to, so 1 and 1 leave it alone; the axis directions and
       the sign of the difference are the part that has to be checked against a rendered result,
       and this is where a flip belongs when it turns out to be needed. */
    float output_scale_x;
    float output_scale_y;
    /* Written wherever the source held the clear value. Must be outside any real motion, and
       representable in the output format: the default -1000 is both, in half precision. This is
       the value to hand a backend as its "no motion vector here" marker. */
    float invalid_value;
    /* Whether a stored zero means "nothing wrote this pixel". True for Unreal. When false, no
       pixel is marked and `invalid_value` is unused. */
    uint32_t zero_means_unwritten;
} rsf_motion_decode_params;

typedef struct rsf_motion_decode rsf_motion_decode;

/* Build the pass for one render size. `device` is an `ID3D11Device*`. */
rsf_motion_decode_result rsf_motion_decode_create(void* device,
                                                  const rsf_motion_decode_setup* setup,
                                                  rsf_motion_decode** out);

/* Decode `source` into the pass's own target. `context` is an `ID3D11DeviceContext*` and `source`
   an `ID3D11Texture2D*` of the size this pass was built for.

   Compute state is saved and restored around the dispatch, because this runs inside a frame the
   game is in the middle of and did not ask for its bindings to change. Nothing else is touched. */
rsf_motion_decode_result rsf_motion_decode_run(rsf_motion_decode* pass, void* context,
                                               void* source,
                                               const rsf_motion_decode_params* params);

/* The decoded target, as an `ID3D11Texture2D*`. Owned by the pass, valid until it is destroyed,
   and not reference counted for the caller. */
void* rsf_motion_decode_texture(rsf_motion_decode* pass);

void rsf_motion_decode_destroy(rsf_motion_decode* pass);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_MOTION_DECODE_H */
