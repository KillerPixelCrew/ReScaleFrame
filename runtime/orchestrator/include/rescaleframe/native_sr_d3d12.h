// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/game_renderer.h>

typedef struct rsf_sr12 rsf_sr12;
/* Native D3D12 normalization/SR owner. ABI is 1. backend is 0 for input normalization, 1 for
   DLSS, or an rsf_sr_backend family. quality uses the shared backend quality numbering.
   Device is retained; log/user and an optional shared Streamline host outlive the owner. */
typedef struct rsf_sr12_setup {
    uint32_t struct_size, abi_version;
    void* device;
    uint32_t backend, quality, output_width, output_height;
    uint32_t inverted_depth;
    const char* fsr2_directory_utf8;
    const char* fsr3_directory_utf8;
    const char* fsr4_directory_utf8;
    const char* xess_directory_utf8;
    rsf_dlss_setup dlss;
    rsf_backend_log_fn log;
    void* user;
    /* Backend zero normalizes inputs only. These extents may differ from output (FSR1). */
    uint32_t render_width, render_height;
    void* streamline_host;
} rsf_sr12_setup;

#ifdef __cplusplus
extern "C" {
#endif
RSF_RUNTIME_API rsf_backend_result rsf_sr12_create(const rsf_sr12_setup*, rsf_sr12**);
/* Copy the selected render dimensions in pixels; no device or vendor calls are made. */
RSF_RUNTIME_API rsf_backend_result rsf_sr12_plan(const rsf_sr12*, uint32_t* width, uint32_t* height);
// Input states NON_PIXEL_SHADER_RESOURCE, game output COPY_DEST. The private staging output
// reaches the engine only on accepted evaluation. Caller drains the slot before reusing its
// descriptors, and all submitted work before resize/destroy. Command slot is 0..2.
// Origin-zero inputs are normalized to RGBA16_FLOAT color, R32_FLOAT depth and R16G16_FLOAT
// previous-minus-current render-pixel motion. This records commands without submitting them
// and leaves caller command-list bindings changed. Success also records independent FG copies.
// Captured FG record.frame_id uses pass.native_frame; integrations must make that ID match their
// CPU/window source_frame_id to use this automatic capture for interpolation.
RSF_RUNTIME_API rsf_backend_result rsf_sr12_evaluate(rsf_sr12*, void* command_list,
    const rsf_game_render_pass*, uint32_t command_slot);
/* Null is allowed. Caller completes all recorded/submitted uses before releasing this owner. */
RSF_RUNTIME_API void rsf_sr12_destroy(rsf_sr12*);
#ifdef __cplusplus
}
#endif
