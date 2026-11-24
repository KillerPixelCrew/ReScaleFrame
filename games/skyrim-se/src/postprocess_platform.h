// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// The direct Skyrim call and the Community Shaders HDR wrapper share this call shape.
typedef struct rsf_skyrim_cs_hdr_call {
    uintptr_t caller, manager, argument4;
    uint32_t input, output;
    uint8_t argument5;
} rsf_skyrim_cs_hdr_call;
typedef struct rsf_skyrim_taa_receipt {
    uintptr_t manager, taa;
    uint8_t enabled;
} rsf_skyrim_taa_receipt;

_Static_assert(sizeof(rsf_skyrim_cs_hdr_call) == 40 && offsetof(rsf_skyrim_cs_hdr_call, argument5) == 32,
    "Skyrim HDR call layout");
_Static_assert(sizeof(rsf_skyrim_taa_receipt) == 24 && offsetof(rsf_skyrim_taa_receipt, enabled) == 16,
    "Skyrim TAA receipt layout");

void rsf_skyrim_postprocess_prepare(const rsf_skyrim_cs_hdr_call*, rsf_skyrim_taa_receipt*);
void rsf_skyrim_postprocess_restore(const rsf_skyrim_taa_receipt*);
