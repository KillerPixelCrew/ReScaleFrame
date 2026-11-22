// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#include "../../common/frame_capture12.h"

extern "C" const rsf_generation_provider* rsf_core_fsr12_native_provider(const rsf_gen12_checks*,
    const rsf_core_capture12_platform*);
extern "C" uint32_t rsf_core_fsr12_native_supported(const rsf_gen12_checks*, void*, const char*);

extern "C" const rsf_generation_provider* rsf_generation_fsr()
{
#if defined(RSF_HAVE_FFX_FG)
    return rsf_core_fsr12_native_provider(rsf_core_fg12_generation_checks(), rsf_core_capture12_native_platform());
#else
    return rsf_core_fsr12_native_provider(nullptr, nullptr);
#endif
}

extern "C" uint32_t rsf_generation_fsr4_supported(void* device, const char* directory)
{
#if defined(RSF_HAVE_FFX_FG)
    return rsf_core_fsr12_native_supported(rsf_core_fg12_generation_checks(), device, directory);
#else
    return rsf_core_fsr12_native_supported(nullptr, device, directory);
#endif
}
