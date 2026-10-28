// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/runtime.h>
#include <rescaleframe/sr_session.h>
#include <rescaleframe/dlss.h>
#include <rescaleframe/game_renderer.h>

typedef struct rsf_sr12 rsf_sr12;
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
    /* Zero preserves the existing HDR contract; OpenGL SDR scenes set this to one. */
    uint32_t input_sdr;
    /* ABI 2 only: use the engine's active input extents instead of optimal-size rounding.
       Vendor contexts still use the selected quality and output extent. Dynamic input sizes
       must be supported by that provider. ABI 1 retains its original planning behavior. */
    uint32_t engine_input_extents;
    /* ABI 2: consume the first component of a 1x1 floating-point pass.exposure,
       NON_PIXEL_SHADER_RESOURCE, normalized to R32_FLOAT for every backend.
       Fixed for the lifetime of this context. */
    uint32_t external_exposure;
    /* ABI 2: borrowed rsf_dlss_native12_registration, retained by its per-feature context.
       Keeps the game's native NGX handles alive while SR backends/presets/resolution change. */
    void* dlss_registration;
} rsf_sr12_setup;

#ifdef __cplusplus
extern "C" {
#endif
RSF_RUNTIME_API rsf_backend_result rsf_sr12_create(const rsf_sr12_setup*, rsf_sr12**);
RSF_RUNTIME_API rsf_backend_result rsf_sr12_plan(const rsf_sr12*, uint32_t* width, uint32_t* height);
// Input states NON_PIXEL_SHADER_RESOURCE, game output COPY_DEST. The private staging output
// reaches the engine only on accepted evaluation. Caller drains the slot before reusing its
// descriptors, and all submitted work before resize/destroy. Command slot is 0..2.
RSF_RUNTIME_API rsf_backend_result rsf_sr12_evaluate(rsf_sr12*, void* command_list,
    const rsf_game_render_pass*, uint32_t command_slot);
RSF_RUNTIME_API void rsf_sr12_destroy(rsf_sr12*);
RSF_RUNTIME_API rsf_backend_result rsf_sr12_present(rsf_sr12*, void* swapchain,
    uint32_t sync_interval, uint32_t flags, uint32_t begin);
/* Optional bounded original-pixel evidence. Configure before evaluation; the ordinary slot
   completion protocol also retires these readbacks. No work is recorded when unconfigured. */
RSF_RUNTIME_API rsf_backend_result rsf_sr12_capture(rsf_sr12*, const char* directory_utf8,
    uint64_t first_frame, uint32_t frames);
/* Caller has observed this slot's submitted fence completion, including failed evaluations. */
RSF_RUNTIME_API rsf_backend_result rsf_sr12_capture_flush(rsf_sr12*, uint32_t command_slot);
/* Borrow the accepted SR slot's copied camera/frame identity for a later scene-output copy on
   the same still-unsubmitted command-list recording. The ordinary slot fence retires it. */
RSF_RUNTIME_API rsf_backend_result rsf_sr12_capture_scene(rsf_sr12*, void* command_list,
    const rsf_game_render_pass*, uint32_t command_slot);
#ifdef __cplusplus
}
#endif
