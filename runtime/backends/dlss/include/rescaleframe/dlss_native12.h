// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/streamline_host.h>

typedef struct rsf_dlss_native12 rsf_dlss_native12;
typedef struct rsf_dlss_native12_registration rsf_dlss_native12_registration;
/* Keep NGX/Streamline initialization alive across SR-context changes when a native game also
   owns NGX handles. Context destruction frees only its feature resources. The registration
   outlives every borrowed context and the game's own NGX resources. */
rsf_backend_result rsf_dlss_native12_registration_create(void* device, const rsf_dlss_setup*, rsf_dlss_native12_registration**);
// Borrow the already initialized engine SDK. Never initializes, registers another
// device, shuts down, or performs Present bookkeeping for that SDK.
rsf_backend_result rsf_dlss_native12_registration_borrow(void* device, void* module,
    const rsf_dlss_setup*, rsf_dlss_native12_registration**);
rsf_backend_result rsf_dlss_native12_create_registered(void* device, const rsf_dlss_setup*, rsf_dlss_native12_registration*, rsf_dlss_native12**);
rsf_backend_result rsf_dlss_native12_registration_present(rsf_dlss_native12_registration*, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin);
void rsf_dlss_native12_registration_destroy(rsf_dlss_native12_registration*);
rsf_backend_result rsf_dlss_native12_create(void* device, const rsf_dlss_setup* setup, rsf_dlss_native12** out);
rsf_backend_result rsf_dlss_native12_create_shared(void* device, const rsf_dlss_setup*, rsf_streamline_host*, rsf_dlss_native12**);
rsf_backend_result rsf_dlss_native12_plan(rsf_dlss_native12* context, uint32_t width, uint32_t height,
    rsf_quality quality, uint32_t* render_width, uint32_t* render_height);
// D3D12 inputs are NON_PIXEL_SHADER_RESOURCE, output UNORDERED_ACCESS. Caller retires GPU work
// before destroy. This context uses the supplied device and never creates/upgrades another one.
rsf_backend_result rsf_dlss_native12_evaluate(rsf_dlss_native12* context, void* command_list, const rsf_sr_frame* frame);
rsf_backend_result rsf_dlss_native12_evaluate_borrowed(rsf_dlss_native12*, void* command_list,
    const rsf_sr_frame*, void* native_frame_token);
void rsf_dlss_native12_destroy(rsf_dlss_native12* context);
void rsf_dlss_native12_set_sdr(rsf_dlss_native12* context, uint32_t sdr);
// Manual-hooking garbage collection around the actual native Present. Shared contexts leave
// presentation to their common host. No swapchain/device interface is upgraded here.
rsf_backend_result rsf_dlss_native12_present(rsf_dlss_native12*, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin);
