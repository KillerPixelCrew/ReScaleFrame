/* SPDX-License-Identifier: GPL-3.0-only */
/* Work out what a texture is for, from the descriptor it was created with.

   A backend needs four specific targets out of the several hundred a frame allocates. Frame capture
   resource identifiers do not exist at runtime, so the only thing available when a texture appears
   is its descriptor: format, size, and what it may be bound as.

   That is enough for three of the four, and honestly not enough for the fourth. Velocity, depth and
   the exposure target each have a shape nothing else in the frame shares. Scene colour does not:
   Ace Combat 7 allocates many full resolution `R16G16B16A16_FLOAT` render targets, and which one a
   reconstruction wants is decided by what it is bound alongside, which a descriptor cannot say. So
   scene colour comes back marked as a candidate rather than picked, and the caller disambiguates
   from the bound set. Being right by accident on the frame someone happened to test is the failure
   this distinction exists to prevent.

   The counterpart to that honesty is where the certainty comes from. `docs/research/ac7-frame-
   capture.md` establishes the formats and sizes from a replayed capture, not from what stock Unreal
   would do, which is a different engine branch and was wrong about other things. */

#ifndef RSF_RESOURCE_ROLES_H
#define RSF_RESOURCE_ROLES_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* No ABI version here, deliberately. Classification is a pure function over a descriptor with no
   state and no handle, so the only compatibility question is the shape of the structures, which
   their `struct_size` fields already answer. Declaring a version nothing checks would be worse than
   declaring none. */

typedef uint32_t rsf_resource_role;
#define RSF_ROLE_UNKNOWN ((rsf_resource_role)0)
/* Pre-tonemap scene colour, the input a super resolution pass reconstructs from. */
#define RSF_ROLE_SCENE_COLOR ((rsf_resource_role)1)
#define RSF_ROLE_DEPTH ((rsf_resource_role)2)
/* The engine's velocity target, still in its own encoding. */
#define RSF_ROLE_MOTION ((rsf_resource_role)3)
/* The 1x1 target the tonemapper's eye adaptation writes. */
#define RSF_ROLE_EXPOSURE ((rsf_resource_role)4)

/* How much the descriptor alone settles. */
typedef uint32_t rsf_role_confidence;
/* Nothing else in the frame has this shape. */
#define RSF_ROLE_CONFIDENT ((rsf_role_confidence)0)
/* The shape fits, and so does that of other textures. Needs disambiguating by what it is bound
   with, which a descriptor cannot tell you. */
#define RSF_ROLE_CANDIDATE ((rsf_role_confidence)1)

/* What `ID3D11Device::CreateTexture2D` was given, in the fields that matter here. Passed as plain
   numbers so this header stays free of the D3D11 headers; the values are DXGI_FORMAT and
   D3D11_BIND_FLAG as they come. */
typedef struct rsf_texture_facts {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t bind_flags;
    uint32_t mip_levels;
    uint32_t array_size;
    uint32_t sample_count;
} rsf_texture_facts;

/* What the frame looks like, so sizes can be judged relative to it rather than absolutely.

   Render and output resolution are equal until a render scale is applied, which is exactly the
   thing this project makes happen, so both are named.

   A zero render size means it is not known yet. Rather than accepting anything smaller than the
   output, which lets the game's 1024x1024 shadow cascades pass as depth at full confidence, an
   unknown render size accepts a target from half the output size upwards whose aspect ratio matches
   the output's. A scaled render target keeps the frame's aspect; a shadow map does not. */
typedef struct rsf_frame_shape {
    uint32_t struct_size;
    uint32_t output_width;
    uint32_t output_height;
    uint32_t render_width;
    uint32_t render_height;
} rsf_frame_shape;

typedef struct rsf_role_verdict {
    uint32_t struct_size;
    rsf_resource_role role;
    rsf_role_confidence confidence;
} rsf_role_verdict;

/* Classify one texture. Returns zero and leaves `verdict` at unknown if the arguments are short. */
uint32_t rsf_classify_texture(const rsf_texture_facts* facts, const rsf_frame_shape* shape,
                              rsf_role_verdict* verdict);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_RESOURCE_ROLES_H */
