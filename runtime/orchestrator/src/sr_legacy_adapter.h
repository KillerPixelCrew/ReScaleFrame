// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/dlss.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/motion_resolve.h>
struct rsf_sr_legacy_adapter;
rsf_backend_result rsf_sr_legacy_create(void* device, uint32_t width, uint32_t height,
    const char* fsr2, const char* fsr3, const char* fsr4, const char* xess,
    float units_to_meters, rsf_backend_log_fn log, void* user, rsf_sr_legacy_adapter** out);
rsf_backend_result rsf_sr_legacy_select(rsf_sr_legacy_adapter* adapter, uint32_t backend,
    rsf_quality quality, uint32_t* width, uint32_t* height);
/* `frame` motion times its motion scale is current-minus-previous UV, the pipeline's canonical unit. */
rsf_backend_result rsf_sr_legacy_evaluate(rsf_sr_legacy_adapter* adapter, void* context,
    const rsf_dlss_frame* frame, uint32_t has_sentinel);
void rsf_sr_legacy_destroy(rsf_sr_legacy_adapter* adapter);
/* Borrowed normalized current-frame inputs, valid until the next evaluation/destroy. */
int rsf_sr_legacy_fg_inputs(rsf_sr_legacy_adapter*, void** depth, void** motion);

namespace rsf {
/* Keep a motion resolve pass built for the render size, rebuilding it when the size moves. A failed
   rebuild keeps the previous pass and returns false. `rebuilt` says whether it changed. */
inline bool fit_motion_resolve(void* device, uint32_t width, uint32_t height, rsf_motion_resolve*& resolve,
                               uint32_t& resolve_width, uint32_t& resolve_height, bool* rebuilt = nullptr)
{
    if (rebuilt) *rebuilt = false;
    if (resolve && resolve_width == width && resolve_height == height) return true;
    rsf_motion_resolve* replacement = nullptr;
    if (!rsf_motion_resolve_create(device, width, height, &replacement)) return false;
    rsf_motion_resolve_destroy(resolve); resolve = replacement;
    resolve_width = width; resolve_height = height;
    if (rebuilt) *rebuilt = true;
    return true;
}
}
