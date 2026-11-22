/* SPDX-License-Identifier: MIT */
/* The preset validator is header-inline C, so Rust reaches it through this wrapper. */
#include "unity_sr_d3d12_abi.h"
#include <rescaleframe/dlss_preset.h>

int rsf_fixture_unity_dlss_preset_valid(uint32_t preset)
{
    return rsf_dlss_preset_valid(preset);
}
