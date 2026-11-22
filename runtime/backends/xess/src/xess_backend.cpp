// SPDX-License-Identifier: MIT
#include <rescaleframe/xess_backend.h>
extern "C" const rsf_sr_provider* rsf_core_d3d12_sr_provider(uint32_t,
    decltype(rsf_sr_provider::open), decltype(rsf_sr_provider::evaluate));
extern "C" const rsf_fg_provider* rsf_core_d3d12_legacy_fg_provider(uint32_t, uint32_t);

#if defined(RSF_HAVE_XESS)
#include "../../common/sr12_platform.h"
#include "../../common/sr_helpers.h"

namespace {
void* load(const rsf_sr_open_desc* desc)
{
    return rsf::load_runtime(*desc, L"libxess.dll");
}

} // namespace

extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_xess_open(
    const rsf_sr_open_desc*, void**, void* (*)(const rsf_sr_open_desc*));

namespace {

rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    const auto result = rsf::validate_open(desc, out);
    return result == RSF_BACKEND_OK ? rsf_core_d3d12_sr_provider_xess_open(desc, out, load) : result;
}

} // namespace
#endif

extern "C" const rsf_sr_provider* rsf_xess_sr_provider()
{
#if defined(RSF_HAVE_XESS)
    static const bool bound = (rsf_core_d3d12_sr_bind_validation(rsf::validate_frame,
        [](const rsf_sr_frame* input, void* device) { return rsf::validate_d3d12_resources(*input, device); }), true);
    (void)bound;
    return rsf_core_d3d12_sr_provider(0, sr_open, rsf_core_d3d12_sr_provider_evaluate);
#else
    return rsf_core_d3d12_sr_provider(0, nullptr, nullptr);
#endif
}

extern "C" const rsf_fg_provider* rsf_xess_fg_provider()
{
#if defined(RSF_HAVE_XESS)
    return rsf_core_d3d12_legacy_fg_provider(0, 1);
#else
    return rsf_core_d3d12_legacy_fg_provider(0, 0);
#endif
}
