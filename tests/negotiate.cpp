/* SPDX-License-Identifier: GPL-3.0-only */
/* Choosing between three vendors, on a machine that has none of them.
 *
 * This is the point of negotiation being a pure function. The combinations that matter are the ones
 * nobody has the hardware to try: an NVIDIA card asked for FSR upscaling and DLSS generation, an
 * Intel card where XeSS reconstructs natively and nothing else does, a machine where the bridge
 * failed to come up and every D3D12 vendor has to be refused rather than chosen and then found
 * impossible. All of them are decided from capability structures, so all of them are testable here.
 *
 * What is being pinned is not the choice so much as the reasons. A user who asks for two vendors and
 * gets one needs to know which constraint took the other away.
 */
#include <rescaleframe/backend_registry.h>

#include <cstdio>

namespace {

bool passed = true;

void check(bool condition, const char* what)
{
    if (!condition) {
        std::fprintf(stderr, "FAIL: %s\n", what);
        passed = false;
    }
}

void stage(const char* what) { std::fprintf(stderr, "[stage] %s\n", what); }

/* Streamline: reconstruction on D3D11 and D3D12, generation on D3D12, and one session for both, so
   choosing it for generation drags reconstruction onto D3D12 with it. */
rsf_backend_caps dlss()
{
    rsf_backend_caps caps{};
    caps.struct_size = sizeof(caps);
    caps.vendor = RSF_VENDOR_NVIDIA;
    caps.name = "DLSS";
    caps.available = 1;
    caps.sr_apis = RSF_API_D3D11 | RSF_API_D3D12;
    caps.fg_apis = RSF_API_D3D12;
    caps.max_generated_frames = 3;
    caps.ui_modes = RSF_UI_MODE_UI_LAYER | RSF_UI_MODE_BACKBUFFER_HUDLESS_UI;
    caps.fg_owns_swapchain = 1;
    caps.sr_fg_share_session = 1;
    caps.latency_modes = RSF_LATENCY_PCL;
    caps.fg_requires_latency_markers = 1;
    return caps;
}

/* FidelityFX: D3D12 only for both, and no shared session. */
rsf_backend_caps fsr()
{
    rsf_backend_caps caps{};
    caps.struct_size = sizeof(caps);
    caps.vendor = RSF_VENDOR_AMD;
    caps.name = "FSR";
    caps.available = 1;
    caps.sr_apis = RSF_API_D3D12;
    caps.fg_apis = RSF_API_D3D12;
    caps.max_generated_frames = 1;
    caps.ui_modes = RSF_UI_MODE_UI_LAYER | RSF_UI_MODE_BACKBUFFER_HUDLESS;
    caps.fg_owns_swapchain = 1;
    return caps;
}

/* XeSS: reconstruction on D3D11 as well, which on Intel hardware is the only native D3D11 path any
   vendor offers. */
rsf_backend_caps xess(uint32_t d3d11_sr)
{
    rsf_backend_caps caps{};
    caps.struct_size = sizeof(caps);
    caps.vendor = RSF_VENDOR_INTEL;
    caps.name = "XeSS";
    caps.available = 1;
    caps.sr_apis = d3d11_sr ? (RSF_API_D3D11 | RSF_API_D3D12) : RSF_API_D3D12;
    caps.fg_apis = RSF_API_D3D12;
    caps.max_generated_frames = 3;
    caps.ui_modes = RSF_UI_MODE_BACKBUFFER_HUDLESS_UI;
    caps.fg_owns_swapchain = 1;
    caps.latency_modes = RSF_LATENCY_XELL;
    return caps;
}

rsf_negotiate_request ask()
{
    rsf_negotiate_request request{};
    request.struct_size = sizeof(request);
    request.abi_version = RSF_BACKEND_REGISTRY_ABI_VERSION;
    request.game_api = RSF_API_D3D11;
    request.bridge_available = 1;
    request.want_sr = 1;
    request.fg_multiplier = 2;
    return request;
}

} // namespace

int main()
{
    rsf_backend_choice choice{};
    choice.struct_size = sizeof(choice);

    stage("reconstruction alone stays where the game is");
    {
        const rsf_backend_caps caps[] = {dlss(), fsr(), xess(0)};
        rsf_negotiate_request request = ask();
        request.sr_vendor = RSF_VENDOR_NVIDIA;
        check(rsf_negotiate(&request, caps, 3, &choice) == RSF_BACKEND_OK, "It must decide.");
        check(choice.sr_vendor == RSF_VENDOR_NVIDIA, "The vendor asked for must be chosen.");
        check(choice.sr_route == RSF_SR_ROUTE_NATIVE_D3D11,
              "And run where the game does, because with no generator there is nothing to force "
              "it across the bridge and the round trip is not free.");
        check(choice.restart_required == 0,
              "Nothing about that needs the presentation chain rebuilt.");
    }

    stage("a shared session drags reconstruction onto the other API");
    {
        // The Streamline case, and the one with a real cost: one graphics API per process, so
        // choosing DLSS-G for generation moves DLSS-SR to D3D12 and the scene colour makes a round
        // trip to be reconstructed on a device the game never used.
        const rsf_backend_caps caps[] = {dlss(), fsr(), xess(0)};
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        request.fg_vendor = RSF_VENDOR_NVIDIA;
        request.sr_vendor = RSF_VENDOR_NVIDIA;
        rsf_negotiate(&request, caps, 3, &choice);
        check(choice.fg_vendor == RSF_VENDOR_NVIDIA && choice.sr_vendor == RSF_VENDOR_NVIDIA,
              "Both from the same vendor.");
        check(choice.sr_route == RSF_SR_ROUTE_BRIDGE_D3D12,
              "And reconstruction moves to D3D12, which is the cost of that vendor's one session "
              "per process.");
        check((choice.reasons & RSF_REASON_SR_FG_SESSION_CONFLICT) != 0,
              "The reason must say so, because otherwise the round trip looks like an accident of "
              "load order.");
        check(choice.restart_required == 1, "And the chain has to be rebuilt for it.");
    }

    stage("asking for two vendors that cannot both be had");
    {
        // FSR upscaling with DLSS generation. Streamline's session takes reconstruction with it, so
        // the FSR request cannot be honoured, and the user is told rather than quietly given
        // something else.
        const rsf_backend_caps caps[] = {dlss(), fsr(), xess(0)};
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        request.fg_vendor = RSF_VENDOR_NVIDIA;
        request.sr_vendor = RSF_VENDOR_AMD;
        rsf_negotiate(&request, caps, 3, &choice);
        check(choice.fg_vendor == RSF_VENDOR_NVIDIA, "Generation is the vendor asked for.");
        check(choice.sr_vendor == RSF_VENDOR_NVIDIA,
              "Reconstruction is not, because the generator owns the session.");
        check((choice.reasons & RSF_REASON_SR_FG_SESSION_CONFLICT) != 0,
              "And the substitution is reported. Silently swapping one vendor for another is the "
              "behaviour this whole structure exists to avoid.");
    }

    stage("a generator with no shared session leaves reconstruction alone");
    {
        const rsf_backend_caps caps[] = {dlss(), fsr(), xess(1)};
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        request.fg_vendor = RSF_VENDOR_AMD;
        request.sr_vendor = RSF_VENDOR_INTEL;
        rsf_negotiate(&request, caps, 3, &choice);
        check(choice.fg_vendor == RSF_VENDOR_AMD && choice.sr_vendor == RSF_VENDOR_INTEL,
              "Both vendors as asked, because nothing forces them together.");
        check(choice.sr_route == RSF_SR_ROUTE_NATIVE_D3D11,
              "And reconstruction stays on D3D11, where XeSS offers it on Intel hardware. Mixing "
              "vendors is allowed; it is only sharing a session that is not.");
    }

    stage("a vendor that offers reconstruction nowhere the game runs");
    {
        const rsf_backend_caps caps[] = {fsr()};
        rsf_negotiate_request request = ask();
        request.sr_vendor = RSF_VENDOR_AMD;
        rsf_negotiate(&request, caps, 1, &choice);
        check(choice.sr_route == RSF_SR_ROUTE_BRIDGE_D3D12,
              "It has to cross the bridge, because it ships for D3D12 only.");
        check((choice.reasons & RSF_REASON_SR_API_UNAVAILABLE) != 0,
              "And the reason must be the API rather than a session conflict: two different causes "
              "produce the same route and a user fixing one needs to know which.");
    }

    stage("no bridge means no generation at all");
    {
        const rsf_backend_caps caps[] = {dlss(), fsr(), xess(1)};
        rsf_negotiate_request request = ask();
        request.bridge_available = 0;
        request.want_fg = 1;
        request.fg_vendor = RSF_VENDOR_AMD;
        request.sr_vendor = RSF_VENDOR_INTEL;
        rsf_negotiate(&request, caps, 3, &choice);
        check(choice.fg_vendor == RSF_VENDOR_NONE,
              "Every vendor generates on D3D12 and the game is D3D11, so without the bridge there "
              "is nowhere to do the work.");
        check((choice.reasons & RSF_REASON_FG_NEEDS_BRIDGE) != 0 &&
                  (choice.reasons & RSF_REASON_NO_FG_AVAILABLE) != 0,
              "Refused here, with the reason, rather than chosen and found impossible three layers "
              "down where the message would be a null pointer.");
        check(choice.sr_vendor == RSF_VENDOR_INTEL && choice.sr_route == RSF_SR_ROUTE_NATIVE_D3D11,
              "Reconstruction still runs, because it never needed the bridge on this hardware.");
    }

    stage("the multiplier is capped by what the vendor grants");
    {
        const rsf_backend_caps caps[] = {fsr()};
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        request.fg_multiplier = 4;
        rsf_negotiate(&request, caps, 1, &choice);
        check(choice.generated_frames == 1,
              "FidelityFX generates one frame between each pair, so four times is not on offer.");
        check((choice.reasons & RSF_REASON_FG_MULTIPLIER_CAPPED) != 0,
              "And saying so is the difference between a counter that reads low and a counter "
              "that reads low for a reason.");
    }

    stage("the interface mode falls back to the closest the vendor takes");
    {
        const rsf_backend_caps caps[] = {fsr()};
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        request.ui_mode = RSF_UI_MODE_BACKBUFFER_HUDLESS_UI;
        rsf_negotiate(&request, caps, 1, &choice);
        check(choice.ui_mode == RSF_UI_MODE_UI_LAYER,
              "Both surfaces are not on offer, so the layer alone is taken, which still keeps the "
              "interface out of the interpolation.");
        check((choice.reasons & RSF_REASON_UI_MODE_UNSUPPORTED) != 0, "And it is reported.");
    }

    stage("nothing available");
    {
        rsf_negotiate_request request = ask();
        request.want_fg = 1;
        rsf_negotiate(&request, nullptr, 0, &choice);
        check(choice.sr_vendor == RSF_VENDOR_NONE && choice.fg_vendor == RSF_VENDOR_NONE,
              "With no vendors there is nothing to choose.");
        check((choice.reasons & RSF_REASON_NO_FG_AVAILABLE) != 0, "And generation says why.");
    }

    stage("an unavailable vendor is not silently replaced without saying so");
    {
        rsf_backend_caps refused = dlss();
        refused.available = 0;
        refused.refusal_utf8 = "no driver";
        const rsf_backend_caps caps[] = {refused, fsr()};
        rsf_negotiate_request request = ask();
        request.sr_vendor = RSF_VENDOR_NVIDIA;
        rsf_negotiate(&request, caps, 2, &choice);
        check(choice.sr_vendor == RSF_VENDOR_AMD, "Another vendor is used.");
        check((choice.reasons & RSF_REASON_REQUESTED_UNAVAILABLE) != 0,
              "And the fact that it is not the one asked for is reported.");
    }

    stage("arguments are checked");
    {
        rsf_negotiate_request request = ask();
        check(rsf_negotiate(nullptr, nullptr, 0, &choice) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
              "A null request.");
        check(rsf_negotiate(&request, nullptr, 0, nullptr) == RSF_BACKEND_ERROR_INVALID_ARGUMENT,
              "A null choice.");
        request.abi_version = RSF_BACKEND_REGISTRY_ABI_VERSION + 1u;
        check(rsf_negotiate(&request, nullptr, 0, &choice) == RSF_BACKEND_ERROR_ABI_MISMATCH,
              "An ABI mismatch.");
        request.abi_version = RSF_BACKEND_REGISTRY_ABI_VERSION;
        rsf_backend_choice short_choice{};
        short_choice.struct_size = 4;
        check(rsf_negotiate(&request, nullptr, 0, &short_choice) ==
                  RSF_BACKEND_ERROR_INVALID_ARGUMENT,
              "A short choice structure.");
    }

    std::fprintf(stderr, "%s\n", passed ? "negotiate: all checks passed" : "negotiate: FAILED");
    return passed ? 0 : 1;
}
