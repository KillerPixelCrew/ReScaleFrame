// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/gpu_policy.h>
// Pure startup policy; the chosen provider still has to pass device/driver/context creation.
uint32_t rsf_gpu_default_sr_backend(uint32_t vendor_id, uint32_t software_adapter)
{
    if (software_adapter) return 0;
    if (vendor_id == 0x10de) return 1;
    if (vendor_id == 0x8086) return 5;
    return 3;
}
