/* SPDX-License-Identifier: GPL-3.0-only */
#ifndef RSF_NAMED_FRAME_DUMP_H
#define RSF_NAMED_FRAME_DUMP_H
#include <rescaleframe/runtime.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_NAMED_FRAME_DUMP_ABI_VERSION 1u

/* One named, opaque PNG (plus its JSON description) of a D3D11 texture, written to a directory the
   caller chooses and outside any frame bundle. It is the core frame dump service's readback and
   encoder (rsf_input_dump_*, texture_dump.h) for adapters that need to look at a presented image
   without a frame record, for example the backbuffer after their overlay drew. Nothing is retained:
   immediate context only, single-sample subresource zero, a staging copy, predication suspended
   around the copy. The caller decides which frames to ask for.

   `context` is the ID3D11DeviceContext* owned by the texture's device and `texture` the
   ID3D11Texture2D*. Exported by ReScaleFrame.Runtime.dll as
   rsf_input_dump_named_texture. This header is self-contained so a DLL adapter needs nothing else. */
typedef void (*rsf_named_frame_dump_log_fn)(void* user, const char* message);

/* How stored values become visible; the values are those of rsf_dump_view in texture_dump.h.
   0 writes channels through scaled from their storage range, 3 is Reinhard+sRGB for linear HDR. */
typedef struct rsf_named_frame_dump {
    uint32_t struct_size, abi_version;
    /* Created when missing. Absolute or process-relative, UTF-8. */
    const char* directory_utf8;
    /* [A-Za-z0-9_-]+. Written as <directory>/<name>.png and .json; an existing file is replaced. */
    const char* name;
    uint32_t view;
    float scale; /* zero means one */
    rsf_named_frame_dump_log_fn log;
    void* log_user;
} rsf_named_frame_dump;

typedef struct rsf_named_frame_dump_report {
    uint32_t struct_size, width, height, dxgi_format;
} rsf_named_frame_dump_report;

/* Returns the rsf_dump_texture_result of the readback: 0 on success, negative on a refusal
   (-1 invalid argument, -2 ABI, -3 unsupported format, -4 staging, -5 map, -6 write, -7 another
   device). `report` is optional and filled on success. */
RSF_RUNTIME_API int32_t rsf_input_dump_named_texture(void* context, void* texture,
    const rsf_named_frame_dump* request, rsf_named_frame_dump_report* report);
typedef int32_t (*rsf_input_dump_named_texture_fn)(void* context, void* texture,
    const rsf_named_frame_dump* request, rsf_named_frame_dump_report* report);
#define RSF_INPUT_DUMP_NAMED_TEXTURE_ENTRY_POINT "rsf_input_dump_named_texture"

#ifdef __cplusplus
}
#endif
#endif
