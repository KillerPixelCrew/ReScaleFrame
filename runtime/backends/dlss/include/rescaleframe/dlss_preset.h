/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <stdint.h>

/* Streamline 2.14.1 model IDs. E/F are legacy models; G/H/I/N/O revert to Default.
   A driver override can replace the application's requested model. */
static inline int rsf_dlss_preset_valid(uint32_t preset)
{
    return preset == 0 || preset == 5 || preset == 6 ||
           (preset >= 10 && preset <= 13);
}
