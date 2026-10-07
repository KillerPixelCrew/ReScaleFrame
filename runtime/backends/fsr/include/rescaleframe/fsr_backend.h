/* SPDX-License-Identifier: GPL-3.0-only */
/** @file FidelityFX provider accessors for the common backend contract. */

#ifndef RSF_FSR_BACKEND_H
#define RSF_FSR_BACKEND_H

#include <rescaleframe/backend.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Return an immutable module-owned SR table. Probe reports compile capability; open checks
 * the DX12 device/runtime and requested FSR2/3/4 family. The caller never frees this table.
 */
const rsf_sr_provider* rsf_fsr_sr_provider(void);
/** Legacy FG table that refuses work. Live FG uses rsf_generation_fsr() in frame_generation.h. */
const rsf_fg_provider* rsf_fsr_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FSR_BACKEND_H */
