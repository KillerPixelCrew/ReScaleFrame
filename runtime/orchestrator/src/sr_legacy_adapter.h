// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/dlss.h>
#include <rescaleframe/sr_session.h>
struct rsf_sr_legacy_adapter;
rsf_backend_result rsf_sr_legacy_create(void* device, uint32_t width, uint32_t height,
    const char* fsr2, const char* fsr3, const char* fsr4, const char* xess,
    float units_to_meters, rsf_backend_log_fn log, void* user, rsf_sr_legacy_adapter** out);
rsf_backend_result rsf_sr_legacy_select(rsf_sr_legacy_adapter* adapter, uint32_t backend,
    rsf_quality quality, uint32_t* width, uint32_t* height);
rsf_backend_result rsf_sr_legacy_evaluate(rsf_sr_legacy_adapter* adapter, void* context,
    const rsf_dlss_frame* frame, uint32_t has_sentinel);
void rsf_sr_legacy_destroy(rsf_sr_legacy_adapter* adapter);
/* Borrowed normalized current-frame inputs, valid until the next evaluation/destroy. */
int rsf_sr_legacy_fg_inputs(rsf_sr_legacy_adapter*, void** depth, void** motion);
