/* SPDX-License-Identifier: GPL-3.0-only */
/** @file XeSS provider accessors for the common backend contract. */

#ifndef RSF_XESS_BACKEND_H
#define RSF_XESS_BACKEND_H

#include <rescaleframe/backend.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Return an immutable module-owned DX12 SR table. Probe reports compile capability; open
 * checks the loaded runtime and device. No native D3D11 path is implemented by this provider.
 */
const rsf_sr_provider* rsf_xess_sr_provider(void);
/** Legacy FG table that refuses work. Live FG uses rsf_generation_xess() in frame_generation.h. */
const rsf_fg_provider* rsf_xess_fg_provider(void);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_XESS_BACKEND_H */
