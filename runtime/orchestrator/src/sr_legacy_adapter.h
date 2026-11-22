// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/dlss.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/motion_resolve.h>
struct rsf_sr_legacy_adapter;
extern "C" rsf_backend_result rsf_sr_legacy_create(void* device, uint32_t width, uint32_t height,
    const char* fsr2, const char* fsr3, const char* fsr4, const char* xess,
    float units_to_meters, uint32_t standard_depth, uint32_t auto_exposure, rsf_backend_log_fn log, void* user,
    rsf_sr_legacy_adapter** out);
extern "C" rsf_backend_result rsf_sr_legacy_select(rsf_sr_legacy_adapter* adapter, uint32_t backend,
    rsf_quality quality, uint32_t* width, uint32_t* height);
/* `frame` motion times its motion scale is current-minus-previous UV, the pipeline's canonical unit. */
extern "C" void rsf_sr_legacy_destroy(rsf_sr_legacy_adapter* adapter);
/* Builds the selected backend's lazily created pipelines (sr_bridge.h, rsf_sr_bridge_warm_up) for the
   given sizes, with the exposure policy the adapter was created with. Run on the render thread
   right after select, before the first frame. */
extern "C" rsf_backend_result rsf_sr_legacy_warm_up(rsf_sr_legacy_adapter* adapter, uint32_t render_width,
    uint32_t render_height, uint32_t output_width, uint32_t output_height);

/* Borrowed normalized current-frame inputs, valid until the next evaluation/destroy. */
extern "C" int rsf_sr_legacy_fg_inputs(rsf_sr_legacy_adapter*, void** depth, void** motion);
