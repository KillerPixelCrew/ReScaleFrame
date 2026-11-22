/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/backend.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private D3D12 SR provider ABI. */
rsf_backend_result rsf_core_d3d12_sr_provider_plan(void*, rsf_quality, uint32_t*, uint32_t*);
rsf_backend_result rsf_core_d3d12_sr_provider_evaluate(void*, void*, const rsf_sr_frame*);
rsf_backend_result rsf_core_d3d12_sr_provider_release(void*);
void rsf_core_d3d12_sr_provider_close(void*);
rsf_backend_result rsf_core_d3d12_sr_provider_version(void*, uint64_t*, const char**);

#ifdef __cplusplus
}
#endif
