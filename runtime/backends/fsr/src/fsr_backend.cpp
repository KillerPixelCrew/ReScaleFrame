/* SPDX-License-Identifier: MIT */
/* FSR2, FSR3 and FSR4 reconstruction through the official versioned DX12 API.
   The legacy frame-generation vtable is retained for contract callers. */

#include <rescaleframe/backend.h>
#include <rescaleframe/fsr_backend.h>

#include "../../common/sr_helpers.h"
#if defined(RSF_HAVE_FFX)
#include "../../common/sr12_platform.h"
#include "fsr4_compat_core.h"
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

// The SDK records below are generated into d3d12_sr/fsr_abi.rs.
static_assert(sizeof(FfxApiResource) == 48 && sizeof(ffxCreateContextDescUpscale) == 48 &&
    sizeof(ffxDispatchDescUpscale) == 432 && sizeof(ffxQueryDescGetVersions) == 56 &&
    sizeof(ffxQueryGetProviderVersion) == 32 &&
    sizeof(ffxQueryDescUpscaleGetRenderResolutionFromQualityMode) == 48);
static_assert(RSF_QUALITY_NATIVE == FFX_UPSCALE_QUALITY_MODE_NATIVEAA &&
    RSF_QUALITY_QUALITY == FFX_UPSCALE_QUALITY_MODE_QUALITY &&
    RSF_QUALITY_BALANCED == FFX_UPSCALE_QUALITY_MODE_BALANCED &&
    RSF_QUALITY_PERFORMANCE == FFX_UPSCALE_QUALITY_MODE_PERFORMANCE &&
    RSF_QUALITY_ULTRA_PERFORMANCE == FFX_UPSCALE_QUALITY_MODE_ULTRA_PERFORMANCE);

void* load(const rsf_sr_open_desc* desc)
{
    return rsf::load_runtime(*desc, L"amd_fidelityfx_upscaler_dx12.dll");
}

const rsf_fsr4_compat_lease* acquire_compatibility(void* module, const rsf_sr_open_desc* desc)
{
    return rsf_fsr4_acquire_int8(static_cast<HMODULE>(module), static_cast<ID3D12Device*>(desc->device),
        desc->log, desc->log_user);
}

void resource(void* texture, uint32_t state, FfxApiResource* out)
{
    *out = ffxApiGetResourceDX12(static_cast<ID3D12Resource*>(texture), state);
}

struct FsrOps {
    decltype(&load) load_library;
    decltype(&acquire_compatibility) acquire_compatibility;
    decltype(&rsf_fsr4_compat_release) release_compatibility;
    decltype(&resource) describe_resource;
};
static_assert(sizeof(FsrOps) == 4 * sizeof(void*));
constexpr FsrOps fsr_ops{load, acquire_compatibility, rsf_fsr4_compat_release, resource};

} // namespace

extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_fsr_open(const rsf_sr_open_desc*, void**, const FsrOps*);

namespace {

rsf_backend_result sr_open_managed(const rsf_sr_open_desc* desc, void** out)
{
    const auto result = rsf::validate_open(desc, out);
    return result == RSF_BACKEND_OK ? rsf_core_d3d12_sr_provider_fsr_open(desc, out, &fsr_ops) : result;
}

rsf_backend_result sr_open(const rsf_sr_open_desc* desc, void** out)
{
    const auto result = sr_open_managed(desc, out);
    if (result == RSF_BACKEND_OK && desc->log) {
        uint64_t version_id = 0;
        const char* name = nullptr;
        if (rsf_core_d3d12_sr_provider_version(*out, &version_id, &name) == RSF_BACKEND_OK && name)
            desc->log(desc->log_user, name);
    }
    return result;
}

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
    sizeof(rsf_sr_provider), probe, sr_open,
#if defined(RSF_HAVE_FFX)
    rsf_core_d3d12_sr_provider_plan, rsf_d3d12_sr_provider_evaluate, rsf_core_d3d12_sr_provider_release,
    rsf_core_d3d12_sr_provider_close, rsf_core_d3d12_sr_provider_version,
#else
    sr_plan, sr_evaluate, sr_release, sr_close, nullptr,
#endif
};

#if defined(RSF_HAVE_FFX)
const rsf_sr_provider managed_sr_provider = [] {
    auto provider = sr_provider;
    provider.open = sr_open_managed;
    return provider;
}();
#endif

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,   fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep, fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider(void) { return &sr_provider; }
// The session announces the committed version after planning succeeds.
extern "C" const rsf_sr_provider* rsf_fsr_managed_sr_provider(void)
{
#if defined(RSF_HAVE_FFX)
    return &managed_sr_provider;
#else
    return &sr_provider;
#endif
}

extern "C" const rsf_fg_provider* rsf_fsr_fg_provider(void) { return &fg_provider; }
