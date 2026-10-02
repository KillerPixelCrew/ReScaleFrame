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
#if defined(RSF_HAVE_FFX)
#include <ffx_api.h>
#include <ffx_upscale.h>
#include <dx12/ffx_api_dx12.h>
#endif

namespace {

void say(const rsf_backend_probe_desc* desc, const char* message)
{
    if (desc && desc->log) {
        desc->log(desc->log_user, message);
    }
}

rsf_backend_result probe(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps)
{
    if (!desc || !caps || desc->struct_size < sizeof(rsf_backend_probe_desc) ||
        caps->struct_size < sizeof(rsf_backend_caps)) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (desc->abi_version != RSF_BACKEND_ABI_VERSION) {
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    }

    const uint32_t size = caps->struct_size;
    *caps = rsf_backend_caps{};
    caps->struct_size = size;
    caps->vendor = RSF_VENDOR_AMD;
    caps->name = "FidelityFX";

#if !defined(RSF_HAVE_FFX)
    caps->available = 0;
    caps->refusal_utf8 = "built without the FidelityFX headers";
    say(desc, "fsr: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    caps->fg_apis = 0;
    caps->max_generated_frames = 0;
    caps->ui_modes = 0;
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;
    caps->fg_owns_swapchain = 0;
    caps->sr_fg_share_session = 0;
    caps->fg_requires_latency_markers = 0;
    caps->supports_dynamic_fg = 0;
    caps->needs_exposure = 0;

    /* Probe reports build capability. Open verifies runtime, adapter and requested version. */
    caps->available = 1;
    say(desc, "fsr: SR compiled in, D3D12 only; frame generation unavailable");
    return RSF_BACKEND_OK;
#endif
}

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

rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result fg_options(void*, uint32_t, uint32_t, rsf_ui_mode)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result fg_tag(void*, void*, const rsf_fg_frame*)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result fg_after_present(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result fg_resize(void*, uint32_t, uint32_t, uint32_t)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result fg_marker(void*, rsf_latency_marker, uint64_t)
{
    return RSF_BACKEND_ERROR_NOT_COMPILED;
}
rsf_backend_result fg_sleep(void*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
rsf_backend_result fg_generated(void*, uint64_t*) { return RSF_BACKEND_ERROR_NOT_COMPILED; }
void fg_destroy(void*) {}

#else

#include "fsr_sr.inl"

rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result fg_options(void*, uint32_t, uint32_t, rsf_ui_mode)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result fg_tag(void*, void*, const rsf_fg_frame*) { return RSF_BACKEND_ERROR_NOT_READY; }
rsf_backend_result fg_after_present(void*) { return RSF_BACKEND_ERROR_NOT_READY; }
rsf_backend_result fg_resize(void*, uint32_t, uint32_t, uint32_t)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result fg_marker(void*, rsf_latency_marker, uint64_t)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result fg_sleep(void*) { return RSF_BACKEND_ERROR_NOT_READY; }
rsf_backend_result fg_generated(void*, uint64_t*) { return RSF_BACKEND_ERROR_NOT_READY; }
void fg_destroy(void*) {}

#endif

const rsf_sr_provider sr_provider = {
    sizeof(rsf_sr_provider), probe, sr_open, sr_plan, sr_evaluate, sr_release, sr_close,
#if defined(RSF_HAVE_FFX)
    sr_version,
#else
    nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,   fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep, fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_fsr_fg_provider(void) { return &fg_provider; }
