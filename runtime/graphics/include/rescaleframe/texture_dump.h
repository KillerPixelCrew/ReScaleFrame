/* SPDX-License-Identifier: GPL-3.0-only */
/* Copy a D3D11 texture off the GPU and write it somewhere it can be looked at.

   Frame captures answer what a target is. This answers what it currently holds, from inside the
   running game and without a capture tool, which is what makes an encoding claim checkable.

   Every entry point takes opaque pointers so the header stays a C ABI contract and callers do not
   need the D3D11 headers. */

#ifndef RSF_TEXTURE_DUMP_H
#define RSF_TEXTURE_DUMP_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_TEXTURE_DUMP_ABI_VERSION 2u

typedef int32_t rsf_dump_texture_result;
#define RSF_TEXTURE_OK ((rsf_dump_texture_result)0)
#define RSF_TEXTURE_ERROR_INVALID_ARGUMENT ((rsf_dump_texture_result)-1)
#define RSF_TEXTURE_ERROR_ABI_MISMATCH ((rsf_dump_texture_result)-2)
#define RSF_TEXTURE_ERROR_UNSUPPORTED_FORMAT ((rsf_dump_texture_result)-3)
#define RSF_TEXTURE_ERROR_STAGING_FAILED ((rsf_dump_texture_result)-4)
#define RSF_TEXTURE_ERROR_MAP_FAILED ((rsf_dump_texture_result)-5)
#define RSF_TEXTURE_ERROR_WRITE_FAILED ((rsf_dump_texture_result)-6)
/* The texture belongs to a different device than the one passed in. Copying across devices is
   invalid, and the runtime creating more than one device is exactly how that happens by
   accident. */
#define RSF_TEXTURE_ERROR_FOREIGN_DEVICE ((rsf_dump_texture_result)-7)

/* Where progress lines go while a dump runs.

   A dump that takes the process down says nothing about where it was, and a returned result code
   never arrives. The sink is called before each step rather than after it, so the last line on
   disk names the step that did not survive. A sink that appends and closes per line is what makes
   that true; buffering it defeats the purpose. Optional, a null sink logs nothing. */
typedef void (*rsf_dump_log_fn)(void* user, const char* message);

/* How to turn stored values into something visible. */
typedef uint32_t rsf_dump_view;
/* Write channels through unchanged, scaled from their storage range to 0..255. */
#define RSF_DUMP_VIEW_RAW ((rsf_dump_view)0)
/* Treat the texture as Unreal velocity: decode, then centre zero motion at mid grey so that
   direction and magnitude are both readable, and mark unwritten pixels. */
#define RSF_DUMP_VIEW_VELOCITY ((rsf_dump_view)1)

typedef struct rsf_texture_dump_options {
    uint32_t struct_size;
    uint32_t abi_version;
    /* Written as a Targa alongside a JSON description using this path without an extension. */
    const char* output_prefix_utf8;
    rsf_dump_view view;
    /* Scale applied before the view maps values to bytes. Zero means one. */
    float scale;
    /* Optional progress sink, and the pointer handed back to it. */
    rsf_dump_log_fn log;
    void* log_user;
} rsf_texture_dump_options;

typedef struct rsf_texture_dump_report {
    uint32_t struct_size;
    uint32_t width;
    uint32_t height;
    uint32_t format;
    uint32_t bytes_written;
    /* For a velocity view, the share of pixels left at the clear value, which is how the
       sentinel convention shows up in real data. */
    float fraction_unwritten;
    float min_x;
    float max_x;
    float min_y;
    float max_y;
} rsf_texture_dump_report;

/* `device`, `context` and `texture` are ID3D11Device*, ID3D11DeviceContext* and
   ID3D11Texture2D*. The texture is copied through a staging resource, so the caller's binding
   state and the texture itself are left alone. */
rsf_dump_texture_result rsf_dump_texture(void* device, void* context, void* texture,
                                         const rsf_texture_dump_options* options,
                                         rsf_texture_dump_report* report);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_TEXTURE_DUMP_H */
