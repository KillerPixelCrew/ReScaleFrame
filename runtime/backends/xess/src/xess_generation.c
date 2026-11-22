/* SPDX-License-Identifier: MIT */
#include <rescaleframe/frame_generation.h>

#if defined(RSF_HAVE_XESS_FG)
#define RSF_NATIVE_FG_COMPILED 1u
#else
#define RSF_NATIVE_FG_COMPILED 0u
#endif

const rsf_generation_provider* rsf_core_xeg12_native_provider(uint32_t compiled);

const rsf_generation_provider* rsf_generation_xess(void)
{
    return rsf_core_xeg12_native_provider(RSF_NATIVE_FG_COMPILED);
}
