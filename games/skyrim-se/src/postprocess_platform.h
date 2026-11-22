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

#ifdef __cplusplus
#define RSF_SKYRIM_NOEXCEPT noexcept
#define RSF_SKYRIM_ASSERT static_assert
extern "C" {
#else
#define RSF_SKYRIM_NOEXCEPT
#define RSF_SKYRIM_ASSERT _Static_assert
#endif
RSF_SKYRIM_ASSERT(sizeof(rsf_skyrim_cs_hdr_call) == 40 && offsetof(rsf_skyrim_cs_hdr_call, argument5) == 32,
    "Skyrim HDR call layout");
RSF_SKYRIM_ASSERT(sizeof(rsf_skyrim_taa_receipt) == 24 && offsetof(rsf_skyrim_taa_receipt, enabled) == 16,
    "Skyrim TAA receipt layout");

void rsf_skyrim_postprocess_prepare(const rsf_skyrim_cs_hdr_call*, rsf_skyrim_taa_receipt*) RSF_SKYRIM_NOEXCEPT;
void rsf_skyrim_postprocess_restore(const rsf_skyrim_taa_receipt*) RSF_SKYRIM_NOEXCEPT;
bool rsf_skyrim_render_cleanup_pending(void) RSF_SKYRIM_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#undef RSF_SKYRIM_ASSERT
#undef RSF_SKYRIM_NOEXCEPT
