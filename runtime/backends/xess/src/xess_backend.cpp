/* SPDX-License-Identifier: GPL-3.0-only */
/* XeSS behind the vendor-neutral contract.
 *
 * The one vendor that reconstructs on D3D11, and the only reason a native path exists at all in a
 * D3D11 game. That path is Intel hardware only, so on the machine this project is developed on it
 * reports D3D12 like the others and the bridge is required; on the Claw it is the route that avoids
 * the bridge entirely. Both are reported by the probe rather than discovered at evaluate time,
 * because negotiation has to decide the route before anything is created.
 *
 * Its frame generator wants both the HUD-less colour and the interface layer, which Intel
 * recommends for Unreal titles and which happens to be exactly what extraction produces. That is
 * worth noting rather than being pleased about: it is the same shape all three ask for, and the
 * reason the interface work is upstream of the vendor work rather than beside it.
 */

#include <rescaleframe/backend.h>
#include <rescaleframe/xess_backend.h>

namespace {

void say(const rsf_backend_probe_desc* desc, const char* message)
{
    if (desc && desc->log) {
        desc->log(desc->log_user, message);
    }
}

/* Whether reconstruction can run where the game does.
 *
 * XeSS's D3D11 build is Intel only, and asking the adapter is the honest way to know. Not
 * implemented yet: until it is, the answer is no, which costs a bridge round trip on hardware that
 * did not need one and never claims a path that turns out not to exist. Wrong in the safe
 * direction, and the probe says which way it erred. */
#if defined(RSF_HAVE_XESS)
bool d3d11_reconstruction_available(const rsf_backend_probe_desc* desc)
{
    (void)desc;
    return false;
}
#endif

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

#if !defined(RSF_HAVE_XESS)
    caps->available = 0;
    caps->refusal_utf8 = "built without the XeSS headers";
    say(desc, "xess: not compiled in");
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#else
    caps->sr_apis = RSF_API_D3D12;
    if (d3d11_reconstruction_available(desc)) {
        caps->sr_apis |= RSF_API_D3D11;
        say(desc, "xess: compiled in, reconstruction available on D3D11");
    } else {
        say(desc, "xess: compiled in, reconstruction on D3D12 only here");
    }
    caps->fg_apis = RSF_API_D3D12;
    /* Up to three generated frames between each pair on Intel hardware. Elsewhere XeFG runs in its
       non-Intel mode and manages one, which is a difference the negotiation caps rather than
       discovers, so a run on this machine reports what it can really do. */
    caps->max_generated_frames = d3d11_reconstruction_available(desc) ? 3u : 1u;
    caps->ui_modes = RSF_UI_MODE_BACKBUFFER_HUDLESS_UI | RSF_UI_MODE_BACKBUFFER_HUDLESS;
    caps->supported_lifetimes =
        (1u << RSF_LIFETIME_ONLY_NOW) | (1u << RSF_LIFETIME_UNTIL_NEXT_PRESENT);
    caps->fg_owns_swapchain = 1;
    caps->sr_fg_share_session = 0;
    caps->latency_modes = RSF_LATENCY_XELL;
    caps->fg_requires_latency_markers = 0;
    caps->supports_dynamic_fg = 1;
    caps->available = 1;
    return RSF_BACKEND_OK;
#endif
}

#if !defined(RSF_HAVE_XESS)
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_COMPILED;
#else
constexpr rsf_backend_result absent = RSF_BACKEND_ERROR_NOT_READY;
#endif

rsf_backend_result sr_open(const rsf_sr_open_desc*, void**) { return absent; }
rsf_backend_result sr_plan(void*, rsf_quality, uint32_t*, uint32_t*) { return absent; }
rsf_backend_result sr_evaluate(void*, void*, const rsf_sr_frame*) { return absent; }
rsf_backend_result sr_release(void*) { return absent; }
void sr_close(void*) {}

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
    sizeof(rsf_sr_provider), probe, sr_open, sr_plan, sr_evaluate, sr_release, sr_close,
};

const rsf_fg_provider fg_provider = {
    sizeof(rsf_fg_provider), probe,     fg_create, fg_options,   fg_tag,       fg_after_present,
    fg_resize,               fg_marker, fg_sleep,  fg_generated, fg_destroy,
};

} // namespace

extern "C" const rsf_sr_provider* rsf_xess_sr_provider(void) { return &sr_provider; }
extern "C" const rsf_fg_provider* rsf_xess_fg_provider(void) { return &fg_provider; }
