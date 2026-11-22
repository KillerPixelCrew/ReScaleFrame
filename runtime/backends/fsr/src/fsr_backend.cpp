// SPDX-License-Identifier: MIT
#include <rescaleframe/fsr_backend.h>
extern "C" const rsf_sr_provider* rsf_core_d3d12_sr_provider(uint32_t,
    decltype(rsf_sr_provider::open), decltype(rsf_sr_provider::evaluate));
extern "C" const rsf_fg_provider* rsf_core_d3d12_legacy_fg_provider(uint32_t, uint32_t);

#if defined(RSF_HAVE_FFX)
#include "../../common/sr12_platform.h"
#include "../../common/sr_helpers.h"
#include "fsr4_compat_core.h"

namespace {
void* load(const rsf_sr_open_desc* desc)
{
    return rsf::load_runtime(*desc, L"amd_fidelityfx_upscaler_dx12.dll");
}

const rsf_fsr4_compat_lease* acquire_compatibility(void* module, const rsf_sr_open_desc* desc)
{
    return rsf_fsr4_acquire_int8(static_cast<HMODULE>(module), static_cast<ID3D12Device*>(desc->device),
        desc->log, desc->log_user);
}

struct FsrOps {
    decltype(&load) load_library;
    decltype(&acquire_compatibility) acquire_compatibility;
    decltype(&rsf_fsr4_compat_release) release_compatibility;
};
static_assert(sizeof(FsrOps) == 3 * sizeof(void*));
constexpr FsrOps fsr_ops{load, acquire_compatibility, rsf_fsr4_compat_release};

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

} // namespace
#endif

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider()
{
#if defined(RSF_HAVE_FFX)
    static const bool bound = (rsf_core_d3d12_sr_bind_validation(rsf::validate_frame,
        [](const rsf_sr_frame* input, void* device) { return rsf::validate_d3d12_resources(*input, device); }), true);
    (void)bound;
    return rsf_core_d3d12_sr_provider(1, sr_open, rsf_core_d3d12_sr_provider_evaluate);
#else
    return rsf_core_d3d12_sr_provider(1, nullptr, nullptr);
#endif
}

extern "C" const rsf_sr_provider* rsf_fsr_managed_sr_provider()
{
    const auto* direct = rsf_fsr_sr_provider();
#if defined(RSF_HAVE_FFX)
    // The session announces the committed version after planning succeeds.
    static const auto provider = [direct] {
        auto value = *direct;
        value.open = sr_open_managed;
        return value;
    }();
    return &provider;
#else
    return direct;
#endif
}

extern "C" const rsf_fg_provider* rsf_fsr_fg_provider()
{
#if defined(RSF_HAVE_FFX)
    return rsf_core_d3d12_legacy_fg_provider(1, 1);
#else
    return rsf_core_d3d12_legacy_fg_provider(1, 0);
#endif
}
