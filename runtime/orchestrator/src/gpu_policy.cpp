// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gpu_policy.h>
#include <rescaleframe/sr_session.h>
uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter)
{
    if (software_adapter) return RSF_SR_NONE;
    if (vendor_id == 0x10de) return RSF_SR_DLSS;
    if (vendor_id == 0x8086) return RSF_SR_XESS;
    return RSF_SR_FSR3;
}
