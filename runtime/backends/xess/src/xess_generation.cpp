// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"

extern "C" const rsf_generation_provider* rsf_core_xeg12_native_provider(const rsf_gen12_checks*);

extern "C" const rsf_generation_provider* rsf_generation_xess()
{
#if defined(RSF_HAVE_XESS_FG)
    return rsf_core_xeg12_native_provider(&rsf::generation12_checks);
#else
    return rsf_core_xeg12_native_provider(nullptr);
#endif
}
