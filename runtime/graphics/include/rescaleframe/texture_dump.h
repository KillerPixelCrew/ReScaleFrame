/* SPDX-License-Identifier: GPL-3.0-only */
/* Synchronous diagnostic D3D11 readback with optional visualisation. Entry points borrow all
   device/context/resource pointers and leave bindings and source contents unchanged. Run on the
   owning immediate-context thread with one device; staging Map can stall the GPU and file writes
   run synchronously. TGA output clips colour and drops source alpha; raw output preserves bytes. */

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

/* Optional synchronous progress sink called before each readback/write stage. Message storage
   lasts only for the call. Flush the sink if logs must identify a crash's last started stage. */
typedef void (*rsf_dump_log_fn)(void* user, const char* message);

/* How to turn stored values into something visible. */
typedef uint32_t rsf_dump_view;
/* Write channels through unchanged, scaled from their storage range to 0..255. */
#define RSF_DUMP_VIEW_RAW ((rsf_dump_view)0)
/* Treat the texture as Unreal velocity: decode, then centre zero motion at mid grey so that
   direction and magnitude are both readable, and mark unwritten pixels. */
#define RSF_DUMP_VIEW_VELOCITY ((rsf_dump_view)1)
/* Motion that a pass has already decoded. Centred the same way as the velocity view so the two can
   be compared side by side, but without decoding again, and marking the sentinel rather than a
   stored zero: after a decode, zero is a real motion of zero and cannot serve as the marker. */
#define RSF_DUMP_VIEW_DECODED_MOTION ((rsf_dump_view)2)
/* Decoded-motion diagnostics treat X at/below this as a sentinel. Choose decoded
   units/scales whose real vectors stay outside that marker range. */
#define RSF_DUMP_DECODED_SENTINEL_THRESHOLD (-100.0f)

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
   ID3D11Texture2D*. The caller supplies the immediate context on device. Reads subresource zero;
   supported MSAA input takes the resolve path. options/report require their documented sizes;
   a null or short report is ignored. Success means the TGA was written; JSON is best effort. */
rsf_dump_texture_result rsf_dump_texture(void* device, void* context, void* texture,
                                         const rsf_texture_dump_options* options,
                                         rsf_texture_dump_report* report);

/* Lossless subresource-zero readback for colour, motion and depth diagnostics. Writes .bin and
   _raw.json with the DXGI format and row pitch. Refuses MSAA, compressed/unknown formats, foreign
   devices and deferred contexts. Rows are packed; source and file row pitches are recorded. */
rsf_dump_texture_result rsf_dump_texture_bytes(void* device, void* context, void* texture,
                                              const rsf_texture_dump_options* options);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_TEXTURE_DUMP_H */
