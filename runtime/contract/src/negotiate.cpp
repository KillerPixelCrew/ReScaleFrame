/* SPDX-License-Identifier: GPL-3.0-only */
#include <rescaleframe/backend_registry.h>

namespace {

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

/* The first available vendor that can do the job, in a fixed order.
 *
 * Order rather than preference: with nothing asked for, the answer has to be the same every run, or
 * a session behaves differently from the last one for reasons nobody can see. */
const rsf_backend_caps* first_with_sr(const rsf_backend_caps* caps, uint32_t count)
{
    for (uint32_t index = 0; index < count; ++index) {
        if (caps[index].available && caps[index].sr_apis != 0) {
            return &caps[index];
        }
    }
    return nullptr;
}

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

/* The closest interface mode this generator accepts.
 *
 * Ordered by how much of the interface problem each one solves. Both surfaces is best, because it is
 * exactly what extraction produces and lets the generator composite a sharp interface onto frames it
 * invents. A HUD-less copy alone still keeps the interface out of the interpolation. None means the
 * interface is interpolated with the scene, which smears it, and is the answer only when there is no
 * other. */
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

    /* Generation first, because it constrains reconstruction and not the other way round: a
       generator that owns the session decides which API the reconstruction runs on. */
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
            /* Every vendor generates on D3D12 and this game renders on D3D11. Without the bridge
               there is nowhere to do the work, so it is refused here rather than chosen and then
               found impossible three layers down. */
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
        /* A generator that shares its session takes the reconstruction with it. Asked before
           falling back to any vendor, because the constraint decides the vendor rather than the
           other way round. */
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
                /* Two different reasons produce the same route and they are worth telling apart: a
                   vendor that never offers D3D11 here, and one that does but cannot use it because
                   the generator owns the session. */
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
        /* Both mean the presentation chain is not the one the game made, and that cannot be
           arranged underneath a running frame. */
        choice->restart_required = 1;
        choice->reasons |= RSF_REASON_NEEDS_RESTART;
    }
    return RSF_BACKEND_OK;
}
