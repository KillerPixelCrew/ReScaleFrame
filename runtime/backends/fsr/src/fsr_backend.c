// SPDX-License-Identifier: MIT
#include <rescaleframe/fsr_backend.h>
#include "fsr4_compat_platform.h"

const rsf_sr_provider* rsf_core_d3d12_fsr_sr_provider(uint32_t,
    const rsf_fsr4_compat_lease* (*)(void*, void*, rsf_fsr4_compat_log_fn, void*),
    void (*)(const rsf_fsr4_compat_lease*));
const rsf_fg_provider* rsf_core_d3d12_legacy_fg_provider(uint32_t, uint32_t);

static const rsf_sr_provider* sr_provider(uint32_t managed)
{
#if defined(RSF_HAVE_FFX)
    return rsf_core_d3d12_fsr_sr_provider(managed,
        rsf_fsr4_compat_acquire_native_device, rsf_fsr4_compat_release);
#else
    return rsf_core_d3d12_fsr_sr_provider(managed, NULL, NULL);
#endif
}
const rsf_sr_provider* rsf_fsr_sr_provider(void) { return sr_provider(0); }
const rsf_sr_provider* rsf_fsr_managed_sr_provider(void) { return sr_provider(1); }

const rsf_fg_provider* rsf_fsr_fg_provider(void)
{
#if defined(RSF_HAVE_FFX)
    return rsf_core_d3d12_legacy_fg_provider(1, 1);
#else
    return rsf_core_d3d12_legacy_fg_provider(1, 0);
#endif
}
