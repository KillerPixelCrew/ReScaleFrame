/* SPDX-License-Identifier: GPL-3.0-only */
/* FidelityFX behind the vendor-neutral contract.
 *
 * The provider exists whether or not the SDK's headers were available at build time. Without them
 * every entry point returns NOT_COMPILED, which is deliberately a different answer from the SDK
 * being present and refusing: one is a fact about how this was built and the other is a fact about
 * the machine, and a user chasing a missing feature needs to know which. That is also why probe
 * fills a refusal string rather than clearing a flag.
 *
 * FidelityFX is the first frame generator to wire up because of what it does not need. It runs on
 * the RTX card in front of this project without vendor lock, it does not require latency markers to
 * place frames, and it does not share a session with its own upscaler, so choosing it for generation
 * leaves reconstruction where it is. DLSS-G and XeFG each add one of those constraints back.
 */

#include <rescaleframe/backend.h>
#include <rescaleframe/fsr_backend.h>

#include <cstring>

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
    /* What FidelityFX is, as against what it is doing here. Filled before anything is loaded,
       because these are properties of the SDK rather than of this machine, and a caller negotiating
       between vendors needs them even when the runtime turns out to be missing.

       Both D3D12 only: ffx_api ships no D3D11 backend, which is why choosing FSR for reconstruction
       means the presentation bridge exists even with generation switched off.

       One generated frame between each pair, so a multiplier above two is capped and said. No
       shared session, so reconstruction is free to be another vendor. No latency markers required,
       which is what lets a frame with an ambiguous identifier still be interpolated around. */
    caps->sr_apis = RSF_API_D3D12;
    caps->fg_apis = RSF_API_D3D12;
    caps->max_generated_frames = 1;
    caps->ui_modes = RSF_UI_MODE_UI_LAYER | RSF_UI_MODE_BACKBUFFER_HUDLESS;
    caps->supported_lifetimes =
        (1u << RSF_LIFETIME_ONLY_NOW) | (1u << RSF_LIFETIME_UNTIL_NEXT_PRESENT);
    caps->fg_owns_swapchain = 1;
    caps->sr_fg_share_session = 0;
    caps->fg_requires_latency_markers = 0;
    caps->supports_dynamic_fg = 1;
    caps->needs_exposure = 0;

    /* The runtime itself is not loaded here. Loading is what `open` and `create_swapchain` do, and
       a probe that loaded would make asking about a vendor cost as much as using it, on a path that
       asks about all three. Availability is therefore a statement about the build and the API, and
       a load failure is reported where it happens with the path that failed. */
    caps->available = 1;
    say(desc, "fsr: compiled in, D3D12 only, one generated frame");
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

/* The implementations land with M6 and M9. They are separated from the capabilities above on
   purpose: negotiation needs to know what FidelityFX is before anything has been created, and that
   question is answerable without the runtime being present at all. */
rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return RSF_BACKEND_ERROR_NOT_READY; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*)
{
    return RSF_BACKEND_ERROR_NOT_READY;
}
rsf_backend_result sr_release(void*) { return RSF_BACKEND_ERROR_NOT_READY; }
void sr_close(void*) {}

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
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,   fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep, fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_fsr_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_fsr_fg_provider(void) { return &fg_provider; }
