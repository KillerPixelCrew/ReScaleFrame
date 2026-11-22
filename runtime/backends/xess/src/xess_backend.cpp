/* SPDX-License-Identifier: MIT */
/* XeSS-SR through the official DX12 API. Native D3D11 and frame generation are unavailable
   in this implementation. Build capability is separate from runtime and adapter support. */

#include <rescaleframe/backend.h>
#include <rescaleframe/xess_backend.h>
#include "../../common/sr_helpers.h"
#if defined(RSF_HAVE_XESS)
#include "../../common/sr12_platform.h"
#include <xess/xess_d3d12.h>
#endif

namespace {

rsf_backend_result probe(const rsf_backend_probe_desc* desc, rsf_backend_caps* caps)
{
    const auto begun = rsf::probe_begin(desc, caps, RSF_VENDOR_INTEL, "XeSS");
    if (begun != RSF_BACKEND_OK) return begun;

#if !defined(RSF_HAVE_XESS)
    caps->available = 0;
    caps->refusal_utf8 = "built without the XeSS headers";
    rsf::probe_say(desc, "xess: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    rsf::probe_say(desc, "xess: SR compiled in, D3D12; native D3D11 and generation unavailable");
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;
    caps->available = 1;
    return RSF_BACKEND_OK;
#endif
}

#if !defined(RSF_HAVE_XESS)
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_COMPILED;
#else
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_READY;
#endif

#if defined(RSF_HAVE_XESS)

// The SDK records below are generated into d3d12_sr/xess_abi.rs.
static_assert(sizeof(xess_version_t) == 8 && sizeof(xess_d3d12_init_params_t) == 64 &&
    sizeof(xess_d3d12_execute_params_t) == 136);
static_assert(RSF_QUALITY_NATIVE == 0 && RSF_QUALITY_QUALITY == 1 && RSF_QUALITY_BALANCED == 2 &&
    RSF_QUALITY_PERFORMANCE == 3 && RSF_QUALITY_ULTRA_PERFORMANCE == 4 && RSF_QUALITY_ULTRA_QUALITY == 5);

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

#else
rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return absent; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*) { return absent; }
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*) { return absent; }
rsf_backend_result sr_release(void*) { return absent; }
void sr_close(void*) {}
#endif

/* Scaffolding: the legacy rsf_fg_provider vtable. Frame generation is reached through
   rsf_generation_xess() in xess_generation.cpp, so these only answer for callers of the old
   contract. */
rsf_backend_result fg_create(const rsf_fg_swapchain_desc*, rsf_fg_swapchain_result*, void**)
{
    return absent;
}
rsf_backend_result fg_options(void*, uint32_t, uint32_t, rsf_ui_mode) { return absent; }
rsf_backend_result fg_tag(void*, void*, const rsf_fg_frame*) { return absent; }
rsf_backend_result fg_after_present(void*) { return absent; }
rsf_backend_result fg_resize(void*, uint32_t, uint32_t, uint32_t) { return absent; }
rsf_backend_result fg_marker(void*, rsf_latency_marker, uint64_t) { return absent; }
rsf_backend_result fg_sleep(void*) { return absent; }
rsf_backend_result fg_generated(void*, uint64_t*) { return absent; }
void fg_destroy(void*) {}

const rsf_sr_provider sr_provider = {
    sizeof(rsf_sr_provider), probe, sr_open,
#if defined(RSF_HAVE_XESS)
    rsf_core_d3d12_sr_provider_plan, rsf_core_d3d12_sr_provider_evaluate, rsf_core_d3d12_sr_provider_release,
    rsf_core_d3d12_sr_provider_close, rsf_core_d3d12_sr_provider_version,
#else
    sr_plan, sr_evaluate, sr_release, sr_close, nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,     fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep,  fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_xess_sr_provider(void)
{
#if defined(RSF_HAVE_XESS)
    static const bool bound = (rsf_core_d3d12_sr_bind_validation(rsf::validate_frame,
        [](const rsf_sr_frame* input, void* device) { return rsf::validate_d3d12_resources(*input, device); }), true);
    (void)bound;
#endif
    return &sr_provider;
}
extern "C" const rsf_fg_provider* rsf_xess_fg_provider(void) { return &fg_provider; }
