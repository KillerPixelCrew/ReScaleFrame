/* SPDX-License-Identifier: MIT */
#include <rescaleframe/frame_generation.h>

#if defined(RSF_HAVE_FFX_FG)
#define RSF_NATIVE_FG_COMPILED 1u
#else
#define RSF_NATIVE_FG_COMPILED 0u
#endif

const rsf_generation_provider* rsf_core_fsr12_native_provider(uint32_t compiled);
uint32_t rsf_core_fsr12_native_supported(uint32_t compiled, void*, const char*);

const rsf_generation_provider* rsf_generation_fsr(void)
{
    return rsf_core_fsr12_native_provider(RSF_NATIVE_FG_COMPILED);
}

uint32_t rsf_generation_fsr4_supported(void* device, const char* directory)
{
    return rsf_core_fsr12_native_supported(RSF_NATIVE_FG_COMPILED, device, directory);
}
