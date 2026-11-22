// SPDX-License-Identifier: MIT
#include "../../common/generation12_checks.h"
#if defined(RSF_HAVE_XESS_FG)
extern "C" rsf_backend_result rsf_core_xeg12_create_native(const rsf_gen12_checks*,
    const rsf_generation_setup*, void**, void**);

extern "C" const rsf_generation_provider* rsf_generation_xess()
{
    static const auto provider = [] {
        auto value = *rsf_core_xeg12_provider();
        value.create = [](const rsf_generation_setup* setup, void** out, void** chain) {
            return rsf_core_xeg12_create_native(&rsf::generation12_checks, setup, out, chain);
        };
        return value;
    }();
    return &provider;
}
#else
extern "C" const rsf_generation_provider* rsf_generation_xess() { return rsf::not_compiled_provider(); }
#endif
