// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Startup preference for the adapter owning the game's graphics device.
   AMD defaults conservatively to FSR3; experimental FSR4 remains a manual choice. */
RSF_RUNTIME_API uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter);
/* Resolves the Auto setting against the backends a host can serve (bit n = backend n).
   An unavailable adapter preference falls back to FSR3, then XeSS, otherwise NONE. */
RSF_RUNTIME_API uint32_t rsf_gpu_select_sr_backend(uint32_t vendor_id, uint32_t software_adapter,
    uint32_t backend_choices);
#ifdef __cplusplus
}
#endif
