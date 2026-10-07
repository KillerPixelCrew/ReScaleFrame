/* SPDX-License-Identifier: GPL-3.0-only */
/* XeSS-SR through the official DX12 API. Native D3D11 and legacy FG are unavailable here;
   xess_generation.cpp implements the separate live-generation API. */

#include <rescaleframe/backend.h>
#include <rescaleframe/xess_backend.h>
#include "../../common/sr_helpers.h"
#include <new>
#if defined(RSF_HAVE_XESS)
#include <xess/xess_d3d12.h>
#endif

namespace {

/* Forward a borrowed diagnostic only when a sink was supplied. */
void say(const rsf_backend_probe_desc* desc, const char* message)
{
    if (desc && desc->log) {
        desc->log(desc->log_user, message);
    }
}

/** Report compiled DX12 SR capability without loading runtimes or querying a device.
 * sr_open establishes actual runtime/adapter support. Output preserves the caller's struct_size.
 */
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
    caps->vendor = RSF_VENDOR_INTEL;
    caps->name = "XeSS";

/* Legacy feature stubs distinguish an absent SDK implementation from an unsupported interface. */
#if !defined(RSF_HAVE_XESS)
    caps->available = 0;
    caps->refusal_utf8 = "built without the XeSS headers";
    say(desc, "xess: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    say(desc, "xess: SR compiled in, D3D12; native D3D11 and generation unavailable");
    caps->fg_apis = 0;
    caps->max_generated_frames = 0;
    caps->ui_modes = 0;
    caps->supported_lifetimes = 1u << RSF_LIFETIME_ONLY_NOW;
    caps->fg_owns_swapchain = 0;
    caps->sr_fg_share_session = 0;
    caps->latency_modes = RSF_LATENCY_NONE;
    caps->fg_requires_latency_markers = 0;
    caps->supports_dynamic_fg = 0;
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
#include "xess_sr.inl"
#else
rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return absent; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*) { return absent; }
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*) { return absent; }
rsf_backend_result sr_release(void*) { return absent; }
void sr_close(void*) {}
#endif

/* All legacy FG operations refuse work; use rsf_generation_xess for XeFG/XeLL. */
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

/* Module-owned immutable tables. Returned pointers require no caller deallocation. */
const rsf_sr_provider sr_provider = {
    sizeof(rsf_sr_provider), probe, sr_open, sr_plan, sr_evaluate, sr_release, sr_close,
#if defined(RSF_HAVE_XESS)
    sr_version,
#else
    nullptr,
#endif
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,     fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep,  fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_xess_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_xess_fg_provider(void) { return &fg_provider; }
