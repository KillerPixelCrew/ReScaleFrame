/* SPDX-License-Identifier: GPL-3.0-only */
/** @file
 * Pure selection of SR and FG providers from caller-supplied capabilities. FG session ownership
 * can force SR onto D3D12; the result identifies that route and every fallback or refusal reason.
 * This contract currently models a D3D11 game with an optional D3D12 presentation bridge.
 */

#ifndef RSF_BACKEND_REGISTRY_H
#define RSF_BACKEND_REGISTRY_H

#include <rescaleframe/backend.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_BACKEND_REGISTRY_ABI_VERSION 1u
/* Number of provider families represented by the built-in registry. */
#define RSF_BACKEND_COUNT 3u

/** Bitmask of selection constraints and fallback reasons; multiple bits may be set. */
typedef uint32_t rsf_negotiate_reason;
#define RSF_REASON_NONE 0x0u
/* The vendor the user asked for is not available here; its own refusal says why. */
#define RSF_REASON_REQUESTED_UNAVAILABLE 0x1u
/* Generation needs the presentation bridge, because every vendor generates on D3D12 and the game
   renders on D3D11. */
#define RSF_REASON_FG_NEEDS_BRIDGE 0x2u
/* SR vendor or route is constrained by the generator's shared session. */
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

/** Selection preferences. Initialize struct_size and RSF_BACKEND_REGISTRY_ABI_VERSION.
 * RSF_VENDOR_NONE means no preference; want_sr/want_fg independently request each feature.
 */
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
    /* Non-zero when the caller can provide a D3D12 presentation bridge. */
    uint32_t bridge_available;
} rsf_negotiate_request;

/** Selected vendors and effective policy. Initialize struct_size before negotiation.
 * A NONE vendor/route denotes an unselected feature. generated_frames counts inserted frames,
 * excluding the source frame. reasons describes changes from the requested policy.
 */
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

/** Choose providers without loading libraries, creating devices, or retaining arguments.
 * caps may be null only when caps_count is zero. The caller supplies initialized capability
 * records and orders them by fallback preference: the first available capable record wins when
 * no explicit vendor is selected. FG is chosen first because a shared session constrains SR.
 * On success choice is cleared and filled, preserving its struct_size. A valid request returns
 * OK even when no feature is selected; inspect choice.reasons and vendor fields for that outcome.
 * Invalid pointers/sizes return INVALID_ARGUMENT; a request version mismatch returns ABI_MISMATCH.
 */
rsf_backend_result rsf_negotiate(const rsf_negotiate_request* request,
                                 const rsf_backend_caps* caps, uint32_t caps_count,
                                 rsf_backend_choice* choice);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_BACKEND_REGISTRY_H */
