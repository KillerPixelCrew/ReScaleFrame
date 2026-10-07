/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/backend_registry.h>

namespace {

/* Find an explicitly requested vendor only if its capability record is available. */
const rsf_backend_caps* find(const rsf_backend_caps* caps, uint32_t count, rsf_vendor vendor)
{
    if (!caps || vendor == RSF_VENDOR_NONE) {
        return nullptr;
    }
    for (uint32_t index = 0; index < count; ++index) {
        if (caps[index].vendor == vendor && caps[index].available) {
            return &caps[index];
        }
    }
    return nullptr;
}

/* Fallback preference is the caller's capability-array order. */
const rsf_backend_caps* first_with_sr(const rsf_backend_caps* caps, uint32_t count)
{
    for (uint32_t index = 0; index < count; ++index) {
        if (caps[index].available && caps[index].sr_apis != 0) {
            return &caps[index];
        }
    }
    return nullptr;
}

/* FG requires an API and at least one generated frame as well as availability. */
const rsf_backend_caps* first_with_fg(const rsf_backend_caps* caps, uint32_t count)
{
    for (uint32_t index = 0; index < count; ++index) {
        if (caps[index].available && caps[index].fg_apis != 0 &&
            caps[index].max_generated_frames > 0) {
            return &caps[index];
        }
    }
    return nullptr;
}

/* Preserve any supported requested bits; otherwise prefer HUD-less plus UI, UI alone,
   HUD-less alone, then scene-only interpolation. */
rsf_ui_mode best_ui_mode(rsf_ui_mode wanted, rsf_ui_mode supported)
{
    const rsf_ui_mode order[] = {RSF_UI_MODE_BACKBUFFER_HUDLESS_UI, RSF_UI_MODE_UI_LAYER,
                                 RSF_UI_MODE_BACKBUFFER_HUDLESS, RSF_UI_MODE_NONE};
    if ((wanted & supported) != 0) {
        return wanted & supported;
    }
    for (rsf_ui_mode candidate : order) {
        if ((supported & candidate) != 0) {
            return candidate;
        }
    }
    return RSF_UI_MODE_NONE;
}

} // namespace

extern "C" rsf_backend_result rsf_negotiate(const rsf_negotiate_request* request,
                                            const rsf_backend_caps* caps, uint32_t caps_count,
                                            rsf_backend_choice* choice)
{
    if (!request || !choice || request->struct_size < sizeof(rsf_negotiate_request) ||
        choice->struct_size < sizeof(rsf_backend_choice)) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }
    if (request->abi_version != RSF_BACKEND_REGISTRY_ABI_VERSION) {
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    }
    if (!caps && caps_count != 0) {
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    }

    const uint32_t size = choice->struct_size;
    *choice = rsf_backend_choice{};
    choice->struct_size = size;
    choice->ui_mode = RSF_UI_MODE_NONE;
    choice->latency = RSF_LATENCY_NONE;

    /* Shared FG sessions constrain the later SR vendor and API selection. */
    const rsf_backend_caps* fg = nullptr;
    if (request->want_fg) {
        fg = find(caps, caps_count, request->fg_vendor);
        if (!fg && request->fg_vendor != RSF_VENDOR_NONE) {
            choice->reasons |= RSF_REASON_REQUESTED_UNAVAILABLE;
        }
        if (!fg) {
            fg = first_with_fg(caps, caps_count);
        }
        if (fg && (fg->fg_apis == 0 || fg->max_generated_frames == 0)) {
            fg = nullptr;
        }
        if (!fg) {
            choice->reasons |= RSF_REASON_NO_FG_AVAILABLE;
        } else if (!request->bridge_available && (fg->fg_apis & RSF_API_D3D11) == 0) {
            /* This route model requires a bridge for providers without a D3D11 FG API. */
            choice->reasons |= RSF_REASON_FG_NEEDS_BRIDGE | RSF_REASON_NO_FG_AVAILABLE;
            fg = nullptr;
        }
    }

    if (fg) {
        choice->fg_vendor = fg->vendor;
        choice->ui_mode = best_ui_mode(request->ui_mode, fg->ui_modes);
        if (request->ui_mode != 0 && choice->ui_mode != request->ui_mode) {
            choice->reasons |= RSF_REASON_UI_MODE_UNSUPPORTED;
        }
        if ((fg->fg_apis & request->game_api) == 0) {
            choice->reasons |= RSF_REASON_FG_NEEDS_BRIDGE;
        }
        /* A multiplier of two means one generated frame between each pair of rendered ones. */
        uint32_t wanted = request->fg_multiplier >= 2u ? request->fg_multiplier - 1u : 1u;
        if (wanted > fg->max_generated_frames) {
            wanted = fg->max_generated_frames;
            choice->reasons |= RSF_REASON_FG_MULTIPLIER_CAPPED;
        }
        choice->generated_frames = wanted;
        if ((fg->latency_modes & request->latency) != 0) {
            choice->latency = request->latency;
        }
    }

    if (request->want_sr) {
        const rsf_backend_caps* sr = find(caps, caps_count, request->sr_vendor);
        if (!sr && request->sr_vendor != RSF_VENDOR_NONE) {
            choice->reasons |= RSF_REASON_REQUESTED_UNAVAILABLE;
        }
        /* Prefer the shared-session partner before consulting the fallback order. */
        if (fg && fg->sr_fg_share_session) {
            const rsf_backend_caps* partner = find(caps, caps_count, fg->vendor);
            if (partner && partner->sr_apis != 0) {
                if (sr && sr->vendor != fg->vendor) {
                    choice->reasons |= RSF_REASON_SR_FG_SESSION_CONFLICT;
                }
                sr = partner;
            }
        }
        if (!sr) {
            sr = first_with_sr(caps, caps_count);
        }
        if (sr) {
            choice->sr_vendor = sr->vendor;
            if ((sr->sr_apis & request->game_api) != 0 && !(fg && fg->sr_fg_share_session)) {
                choice->sr_route = RSF_SR_ROUTE_NATIVE_D3D11;
            } else if (request->bridge_available && (sr->sr_apis & RSF_API_D3D12) != 0) {
                choice->sr_route = RSF_SR_ROUTE_BRIDGE_D3D12;
                /* Distinguish missing native SR support from a shared-session API constraint. */
                if ((sr->sr_apis & request->game_api) == 0) {
                    choice->reasons |= RSF_REASON_SR_API_UNAVAILABLE;
                } else {
                    choice->reasons |= RSF_REASON_SR_FG_SESSION_CONFLICT;
                }
            } else {
                choice->sr_vendor = RSF_VENDOR_NONE;
                choice->sr_route = RSF_SR_ROUTE_NONE;
                choice->reasons |= RSF_REASON_SR_API_UNAVAILABLE;
            }
        }
    }

    if (choice->sr_route == RSF_SR_ROUTE_BRIDGE_D3D12 || choice->fg_vendor != RSF_VENDOR_NONE) {
        /* Bridge SR and any FG selection require the presentation chain to be rebuilt. */
        choice->restart_required = 1;
        choice->reasons |= RSF_REASON_NEEDS_RESTART;
    }
    return RSF_BACKEND_OK;
}
