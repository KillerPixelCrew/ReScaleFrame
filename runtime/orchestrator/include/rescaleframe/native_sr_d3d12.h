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
#ifdef __cplusplus
}
#endif
