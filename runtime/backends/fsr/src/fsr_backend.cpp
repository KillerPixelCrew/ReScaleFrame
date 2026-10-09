/* SPDX-License-Identifier: GPL-3.0-only */
/* FSR2, FSR3 and FSR4 reconstruction through the official versioned DX12 API.
   Frame generation remains a refusing provider; it is never advertised as implemented. */

#include <rescaleframe/backend.h>
#include <rescaleframe/fsr_backend.h>

#include <cstring>
#include <new>
#include <cfloat>
#include "../../common/sr_helpers.h"
#include "fsr4_compat.h"
#include "ffx_version.h"
#if defined(RSF_HAVE_FFX)
#include <ffx_api.h>
#include <ffx_upscale.h>
#include <dx12/ffx_api_dx12.h>
#endif

namespace {

rsf_backend_result probe(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps)
{
    const auto begun = rsf::probe_begin(desc, caps, RSF_VENDOR_AMD, "FidelityFX");
    if (begun != RSF_BACKEND_OK) return begun;

#if !defined(RSF_HAVE_FFX)
    caps->available = 0;
    caps->refusal_utf8 = "built without the FidelityFX headers";
    rsf::probe_say(desc, "fsr: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;

    /* Probe reports build capability. Open verifies runtime, adapter and requested version. */
    caps->available = 1;
    rsf::probe_say(desc, "fsr: SR compiled in, D3D12 only; frame generation unavailable");
    return RSF_BACKEND_OK;
#endif
}

/* Scaffolding: the legacy rsf_fg_provider vtable. Frame generation is reached through
   rsf_generation_fsr() in fsr_generation.cpp, so these only answer for callers of the old contract
   (tests/fsr_backend.cpp). The answer is the same in both builds apart from the code. */
#if !defined(RSF_HAVE_FFX)
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_COMPILED;
#else
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_READY;
#endif

#if !defined(RSF_HAVE_FFX)

rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result sr_release(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
void sr_close(void*) {}

#else

#include "fsr_sr.inl"

#endif

rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**) { return absent; }
rsf_backend_result fg_options(void*, uint32_t, uint32_t, rsf_ui_mode) { return absent; }
rsf_backend_result fg_tag(void*, void*, const rsf_fg_frame*) { return absent; }
rsf_backend_result fg_after_present(void*) { return absent; }
rsf_backend_result fg_resize(void*, uint32_t, uint32_t, uint32_t) { return absent; }
rsf_backend_result fg_marker(void*, rsf_latency_marker, uint64_t) { return absent; }
rsf_backend_result fg_sleep(void*) { return absent; }
rsf_backend_result fg_generated(void*, uint64_t*) { return absent; }
void fg_destroy(void*) {}

const rsf_sr_provider sr_provider = {
    sizeof(rsf_sr_provider), probe, sr_open, sr_plan, sr_evaluate,
#if defined(RSF_HAVE_FFX)
    rsf::sr_release, sr_close, rsf::sr_version<FsrSession>,
#else
    sr_release, sr_close, nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,   fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep, fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_fsr_fg_provider(void) { return &fg_provider; }
