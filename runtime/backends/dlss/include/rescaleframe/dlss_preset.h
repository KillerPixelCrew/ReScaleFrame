/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

/* Streamline 2.14.1 model IDs. E/F are legacy models; G/H/I/N/O revert to Default.
   A driver override can replace the application's requested model. */
#define RSF_DLSS_PRESET_MASK 0x3c61u

static inline int rsf_dlss_preset_valid(uint32_t preset)
{
    return preset <= 13u && (RSF_DLSS_PRESET_MASK & (1u << preset)) != 0;
}
