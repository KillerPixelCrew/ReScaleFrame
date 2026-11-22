// SPDX-License-Identifier: MIT
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

#define RSF_D3D11_SR_ADMISSION_ABI_VERSION 1u
typedef struct rsf_d3d11_sr_admission {
    uint32_t struct_size, abi_version;
    /* Concrete backend bits supported by the caller's rendering path. */
    uint32_t backend_choices;
    uint32_t requested_backend;
    const char* streamline_directory_utf8;
    const char* fidelityfx_directory_utf8;
    const char* xess_directory_utf8;
    /* Borrowed ID3D11Device. NULL requests package inspection before device creation. */
    void* device;
    /* Package evidence only. SDK startup still confirms hardware support. */
    uint32_t offered_backends;
    uint32_t selected_backend;
} rsf_d3d11_sr_admission;
/* On pre-device inspection, the selected backend is Auto when any package is offered.
   On device admission, an offered concrete request wins, otherwise Runtime Auto policy applies. */
RSF_RUNTIME_API int32_t rsf_d3d11_sr_admit(rsf_d3d11_sr_admission*);
#ifdef __cplusplus
}
#endif
