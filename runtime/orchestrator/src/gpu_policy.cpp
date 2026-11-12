// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/sr_session.h>
#include <initializer_list>
uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter)
{
    if (software_adapter) return RSF_SR_NONE;
    if (vendor_id == 0x10de) return RSF_SR_DLSS;
    if (vendor_id == 0x8086) return RSF_SR_XESS;
    return RSF_SR_FSR3;
}
uint32_t rsf_gpu_select_sr_backend(uint32_t vendor_id, uint32_t software_adapter, uint32_t backend_choices)
{
    const auto preferred = rsf_gpu_default_sr_backend(vendor_id, software_adapter);
    if (preferred == RSF_SR_NONE) return RSF_SR_NONE;
    for (const auto backend : {preferred, RSF_SR_FSR3, RSF_SR_XESS})
        if (backend_choices & (1u << backend)) return backend;
    return RSF_SR_NONE;
}
