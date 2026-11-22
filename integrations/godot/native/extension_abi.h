// SPDX-License-Identifier: MIT
#pragma once
#include <stdint.h>

#define RSF_GODOT_PROPERTY_INTEGER 0u
#define RSF_GODOT_PROPERTY_BOOL 1u
#define RSF_GODOT_PROPERTY_REAL 2u
#define RSF_GODOT_PROPERTY_TEXT 3u
#define RSF_GODOT_PROPERTY_EXTENT 4u
#define RSF_GODOT_PROPERTY_TRANSFORM 5u
#define RSF_GODOT_PROPERTY_PROJECTION 6u
#define RSF_GODOT_PROPERTY_RENDERER 7u
#define RSF_GODOT_PROPERTY_DRIVER 8u

typedef union rsf_godot_property {
    int64_t integer;
    double real;
    const char* text;
    int32_t extent[2];
    const double* matrix;
} rsf_godot_property;

/* The payload and any matrix elements are borrowed until the callback returns. */
typedef void (*rsf_godot_property_emit)(void*, const char*, uint32_t, const rsf_godot_property*);

#define RSF_GODOT_MOTION_SHADER 0u
#define RSF_GODOT_MOTION_PIPELINE 1u
#define RSF_GODOT_MOTION_SAMPLER 2u
#define RSF_GODOT_MOTION_BUFFER 3u
#define RSF_GODOT_MOTION_UNIFORMS 4u

typedef struct rsf_godot_effect_readback rsf_godot_effect_readback;
typedef struct rsf_godot_motion_calls {
    uint32_t (*compile)(void*, const rsf_godot_effect_readback*);
    uint32_t (*prepare)(void*, const rsf_godot_effect_readback*, uint32_t*);
    uint32_t (*dispatch)(void*, const rsf_godot_effect_readback*);
    uint32_t (*read)(void*);
    uint32_t (*free_resource)(void*, uint32_t);
} rsf_godot_motion_calls;
