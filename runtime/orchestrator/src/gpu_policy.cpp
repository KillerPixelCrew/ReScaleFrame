// SPDX-License-Identifier: MIT
#include <rescaleframe/gpu_policy.h>
extern "C" uint32_t rsf_core_select_sr_backend(uint32_t vendor, uint32_t software, uint32_t choices);

uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter)
{
    return rsf_core_select_sr_backend(vendor_id, software_adapter, UINT32_MAX);
}
uint32_t rsf_gpu_select_sr_backend(uint32_t vendor_id, uint32_t software_adapter, uint32_t backend_choices)
{
    return rsf_core_select_sr_backend(vendor_id, software_adapter, backend_choices);
}
