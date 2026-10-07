/* SPDX-License-Identifier: GPL-3.0-only */
/* Convert plugin-defined biased motion storage to a backend-readable floating-point field:
   decoded = (stored - bias) * scale * output_scale. The plugin owns units and axis conventions.
   When stored zero marks an unwritten pixel, test it before decoding and write invalid_value
   instead. Outputs belong to the pass and are overwritten by the next run. Call on the owning
   immediate-context thread with resources from the pass's device. */

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

/* Optional synchronous diagnostics; message text is borrowed for the call. */
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
    /* Post-decode axis scale for the backend's units and direction. The plugin supplies it. */
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

/* Build for one render size; retains ID3D11Device until destroy. Recreate to resize. */
rsf_motion_decode_result rsf_motion_decode_create(void* device,
                                                  const rsf_motion_decode_setup* setup,
                                                  rsf_motion_decode** out);

/* Decode `source` into the pass's own target. `context` is an `ID3D11DeviceContext*` and `source`
   an `ID3D11Texture2D*` of the size this pass was built for.

   Saves/restores the compute shader/linkage and SRV/UAV/constant slot 0. The owner must avoid
   conflicting source/output bindings in unsaved stages, which D3D11 may automatically unbind. */
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
