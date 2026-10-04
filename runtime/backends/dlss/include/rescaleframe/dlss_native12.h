// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/streamline_host.h>

typedef struct rsf_dlss_native12 rsf_dlss_native12;
rsf_backend_result rsf_dlss_native12_create(void* device, const rsf_dlss_setup* setup, rsf_dlss_native12** out);
rsf_backend_result rsf_dlss_native12_create_shared(void* device, const rsf_dlss_setup*, rsf_streamline_host*, rsf_dlss_native12**);
rsf_backend_result rsf_dlss_native12_plan(rsf_dlss_native12* context, uint32_t width, uint32_t height,
    rsf_quality quality, uint32_t* render_width, uint32_t* render_height);
// D3D12 inputs are NON_PIXEL_SHADER_RESOURCE, output UNORDERED_ACCESS. Caller retires GPU work
// before destroy. This context uses the supplied device and never creates/upgrades another one.
rsf_backend_result rsf_dlss_native12_evaluate(rsf_dlss_native12* context, void* command_list, const rsf_sr_frame* frame);
void rsf_dlss_native12_destroy(rsf_dlss_native12* context);
