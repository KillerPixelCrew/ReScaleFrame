// SPDX-License-Identifier: MIT
#include <rescaleframe/xess_backend.h>

extern "C" const rsf_sr_provider* rsf_core_d3d12_xess_sr_provider(uint32_t);
extern "C" const rsf_fg_provider* rsf_core_d3d12_legacy_fg_provider(uint32_t, uint32_t);

#if defined(RSF_HAVE_XESS)
static constexpr uint32_t compiled = 1;
#else
static constexpr uint32_t compiled = 0;
#endif

extern "C" const rsf_sr_provider* rsf_xess_sr_provider()
{
    return rsf_core_d3d12_xess_sr_provider(compiled);
}

extern "C" const rsf_fg_provider* rsf_xess_fg_provider()
{
    return rsf_core_d3d12_legacy_fg_provider(0, compiled);
}
