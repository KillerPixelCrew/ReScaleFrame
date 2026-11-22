// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/shader_patch.h>

/* Parsed JSON values in document order. An array or object reports its member count in
   `size`; each object member is a STRING key followed by its value. Integers report their
   64-bit pattern in `number`. Nulls, booleans and reals carry nothing the owner reads. */
enum {
    RSF_PATCH_JSON_OTHER = 0,
    RSF_PATCH_JSON_UINT = 1,
    RSF_PATCH_JSON_SINT = 2,
    RSF_PATCH_JSON_STRING = 3,
    RSF_PATCH_JSON_ARRAY = 4,
    RSF_PATCH_JSON_OBJECT = 5
};
typedef void (*rsf_patch_json_visit)(void* user, uint32_t kind, uint64_t number, const char* text,
                                     size_t size);

int32_t rsf_patch_parse_json(rsf_patch_bytes text, void* user, rsf_patch_json_visit visit);
