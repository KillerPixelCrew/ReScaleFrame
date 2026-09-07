/* SPDX-License-Identifier: GPL-3.0-only */
/* XeSS, behind the contract every vendor implements.
 *
 * Nothing vendor-specific here for the same reason as the other two: everything a caller needs is in
 * `backend.h`, and these getters exist whether or not the SDK was available at build time.
 */

#ifndef RSF_XESS_BACKEND_H
#define RSF_XESS_BACKEND_H

#include <rescaleframe/backend.h>

#ifdef __cplusplus
extern "C" {
#endif

const rsf_sr_provider* rsf_xess_sr_provider(void);
const rsf_fg_provider* rsf_xess_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_XESS_BACKEND_H */
