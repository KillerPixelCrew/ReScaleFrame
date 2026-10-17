// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/backend.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct rsf_gl_context rsf_gl_context;
typedef struct rsf_gl_texture rsf_gl_texture;
#define RSF_GL_RGBA16F 1u
#define RSF_GL_R32F 2u
#define RSF_GL_RG16F 3u
#define RSF_GL_RGBA8 4u
typedef struct rsf_gl_setup {
    uint32_t struct_size, abi_version;
    void* device; // Optional native D3D12 device/queue pair on the GL context's LUID.
    void* queue;
    rsf_backend_log_fn log;
    void* user;
} rsf_gl_setup;
typedef struct rsf_gl_texture_info {
    uint32_t struct_size, texture, width, height, format;
    void* resource;
} rsf_gl_texture_info;
// All calls run on the original WGL context and graphics owner thread.
rsf_backend_result rsf_gl_open(const rsf_gl_setup*, rsf_gl_context**);
rsf_backend_result rsf_gl_graphics(rsf_gl_context*, void** device, void** queue);
rsf_backend_result rsf_gl_texture_create(rsf_gl_context*, uint32_t width, uint32_t height, uint32_t format, rsf_gl_texture**);
rsf_backend_result rsf_gl_texture_get(rsf_gl_texture*, rsf_gl_texture_info*);
// Releases GL textures to D3D12 and returns an open list. Resources enter/leave
// D3D12 in COMMON. Submit even when evaluation refuses, so the GL wait is signalled.
rsf_backend_result rsf_gl_begin(rsf_gl_context*, uint32_t count, rsf_gl_texture* const*, void** list, uint32_t* slot);
rsf_backend_result rsf_gl_submit(rsf_gl_context*, uint32_t count, rsf_gl_texture* const*);
// CPU waits are limited to command-slot reuse and explicit resource retirement.
rsf_backend_result rsf_gl_drain(rsf_gl_context*);
// Drain before releasing textures. A texture does not retain its context.
void rsf_gl_texture_destroy(rsf_gl_texture*);
rsf_backend_result rsf_gl_close(rsf_gl_context*);
#ifdef __cplusplus
}
#endif
