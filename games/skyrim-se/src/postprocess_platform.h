// SPDX-License-Identifier: MIT
#pragma once
#include "native_creation_platform.h"

// The direct Skyrim call and the Community Shaders HDR wrapper share this call shape.
struct rsf_skyrim_cs_hdr_call {
    uintptr_t caller, manager, argument4;
    uint32_t input, output;
    uint8_t argument5;
};
struct rsf_skyrim_taa_receipt {
    uintptr_t manager, taa;
    uint8_t enabled;
};
struct rsf_skyrim_postprocess_leaves {
    bool (*render_allowed)();
    bool (*matches_call)(const rsf_skyrim_cs_hdr_call*);
    void (*fg_inputs)(void*, const rsf_game_host_services*);
    bool (*reconstruct)(void*, const rsf_game_host_services*);
    void (*observe)(const rsf_skyrim_cs_hdr_call*, const rsf_game_host_services*, uint64_t, bool);
};

static_assert(sizeof(rsf_skyrim_cs_hdr_call) == 40 && offsetof(rsf_skyrim_cs_hdr_call, argument5) == 32);
static_assert(sizeof(rsf_skyrim_taa_receipt) == 24 && offsetof(rsf_skyrim_taa_receipt, enabled) == 16);
static_assert(sizeof(rsf_skyrim_postprocess_leaves) == 40);

extern "C" void rsf_skyrim_postprocess_prepare(const rsf_skyrim_creation_platform*,
    const rsf_skyrim_postprocess_leaves*, const rsf_skyrim_cs_hdr_call*, rsf_skyrim_taa_receipt*) noexcept;
extern "C" void rsf_skyrim_postprocess_restore(const rsf_skyrim_creation_platform*,
    const rsf_skyrim_taa_receipt*) noexcept;
extern "C" bool rsf_skyrim_postprocess_busy(const rsf_skyrim_creation_platform*) noexcept;
