// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <stdint.h>
#include <rescaleframe/runtime.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Startup preference for the adapter owning the game's graphics device.
   PCI vendor 0x10de selects DLSS (1), 0x8086 XeSS (5), other hardware FSR3 (3), and software
   selects Off (0). This is a preference, not a capability probe. FSR4 is a manual choice. */
RSF_RUNTIME_API uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter);
#ifdef __cplusplus
}
#endif
