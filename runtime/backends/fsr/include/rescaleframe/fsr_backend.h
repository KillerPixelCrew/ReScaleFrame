/* SPDX-License-Identifier: GPL-3.0-only */
/* FidelityFX, behind the contract every vendor implements.
 *
 * There is nothing vendor-specific in this header on purpose. Everything a caller needs is in
 * `backend.h`, and the only thing this adds is the two getters, which exist whether or not the SDK
 * was available at build time. A caller therefore asks the same question of all three vendors and
 * gets an answer from each, rather than a link error from the ones that are not there.
 */

#ifndef RSF_FSR_BACKEND_H
#define RSF_FSR_BACKEND_H

#include <rescaleframe/backend.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Declared in backend.h alongside the other vendors' and defined here. Repeated in this header so
   that including it is enough, which is what a caller linking only this backend expects. */
const rsf_sr_provider* rsf_fsr_sr_provider(void);
const rsf_fg_provider* rsf_fsr_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_FSR_BACKEND_H */
