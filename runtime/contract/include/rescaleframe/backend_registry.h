/* SPDX-License-Identifier: GPL-3.0-only */
/* Choosing who reconstructs and who generates, and saying why when the answer is nobody.
 *
 * With one vendor this was a question nobody had to ask. With three it is the question, because the
 * choices are not independent:
 *
 *   - Streamline allows one graphics API per process. If DLSS-G generates on D3D12, DLSS-SR cannot
 *     reconstruct on D3D11 in the same process, so the reconstruction moves across the bridge and
 *     pays a round trip it would not otherwise pay.
 *   - FidelityFX ships its upscaler for D3D12 only, so choosing it for reconstruction means the
 *     bridge exists even with generation off.
 *   - XeSS reconstructs on D3D11, but only on Intel hardware. Everywhere else it needs the bridge
 *     too.
 *
 * A user picking "FSR upscaling" and "DLSS frame generation" on an NVIDIA card is asking for
 * something with a specific cost, and the honest thing is to say what it is rather than to refuse or
 * to silently substitute. So this returns a choice with reasons attached, and the reasons are what
 * the overlay shows.
 */

#ifndef RSF_BACKEND_REGISTRY_H
#define RSF_BACKEND_REGISTRY_H

#include <rescaleframe/backend.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_BACKEND_REGISTRY_ABI_VERSION 1u
/* DLSS, FSR, XeSS. Fixed because the set is the set: a fourth vendor is a code change, not a
   configuration. */
#define RSF_BACKEND_COUNT 3u

/* Why the answer is what it is. A bitmask, because several apply at once and a caller showing only
   the first would be telling half a story. */
typedef uint32_t rsf_negotiate_reason;
#define RSF_REASON_NONE 0x0u
/* The vendor the user asked for is not available here; its own refusal says why. */
#define RSF_REASON_REQUESTED_UNAVAILABLE 0x1u
/* Generation needs the presentation bridge, because every vendor generates on D3D12 and the game
   renders on D3D11. */
#define RSF_REASON_FG_NEEDS_BRIDGE 0x2u
/* Reconstruction moved to D3D12 because the chosen generator shares its session, and that session
   can only be one API. This is the Streamline case and the one with a measurable cost. */
#define RSF_REASON_SR_FG_SESSION_CONFLICT 0x4u
/* Reconstruction is on D3D12 because this vendor offers it nowhere else here. */
#define RSF_REASON_SR_API_UNAVAILABLE 0x8u
/* The interface mode asked for is not one this generator accepts; the closest it does is used. */
#define RSF_REASON_UI_MODE_UNSUPPORTED 0x10u
/* Fewer generated frames than were asked for, because the vendor caps it. */
#define RSF_REASON_FG_MULTIPLIER_CAPPED 0x20u
/* The choice cannot be applied without rebuilding the presentation chain. */
#define RSF_REASON_NEEDS_RESTART 0x40u
/* Generation was asked for and no vendor here can do it. */
#define RSF_REASON_NO_FG_AVAILABLE 0x80u

/* What the user asked for. Zero for a vendor means no preference, which is the common case. */
typedef struct rsf_negotiate_request {
    uint32_t struct_size;
    uint32_t abi_version;
    rsf_vendor sr_vendor;
    rsf_vendor fg_vendor;
    uint32_t want_sr;
    uint32_t want_fg;
    /* 2 doubles the presented rate, 3 triples it, and so on. Clamped to what the vendor grants. */
    uint32_t fg_multiplier;
    rsf_ui_mode ui_mode;
    rsf_latency_mode latency;
    /* The API the game renders with, which constrains everything below it. */
    rsf_gfx_api game_api;
    /* Non-zero when the presentation bridge is available. Without it, anything needing D3D12 is
       refused rather than chosen and then found impossible. */
    uint32_t bridge_available;
} rsf_negotiate_request;

typedef struct rsf_backend_choice {
    uint32_t struct_size;
    rsf_vendor sr_vendor;
    rsf_vendor fg_vendor;
    rsf_sr_route sr_route;
    rsf_ui_mode ui_mode;
    rsf_latency_mode latency;
    uint32_t generated_frames;
    uint32_t restart_required;
    rsf_negotiate_reason reasons;
} rsf_backend_choice;

/* Decide, from what each vendor said it could do.
 *
 * Pure: no devices, no loading, no state. `caps` is what `probe` filled for each vendor, indexed
 * however the caller likes; the choice names vendors rather than indices so the ordering carries no
 * meaning. Being pure is what lets the awkward combinations be tested without hardware, which
 * matters because the awkward combinations are the ones nobody has the hardware to try. */
rsf_backend_result rsf_negotiate(const rsf_negotiate_request* request,
                                 const rsf_backend_caps* caps, uint32_t caps_count,
                                 rsf_backend_choice* choice);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_BACKEND_REGISTRY_H */
