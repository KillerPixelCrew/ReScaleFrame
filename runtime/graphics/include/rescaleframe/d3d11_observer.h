/* SPDX-License-Identifier: GPL-3.0-only */
/* Watch a process's D3D11 use without altering what it renders.

   The observer patches two shared vtable entries: texture creation, so render targets can be
   catalogued as they appear, and presentation, so there is a per-frame point to act on. Both
   forward to the original implementation. Nothing here changes a resource, a binding, or a draw.

   Texture creation is the right place to look for a target rather than binding, because it runs
   rarely and carries the full descriptor. Binding runs a hundred times a frame and would put this
   code on the hot path for no extra information.

   Resource identifiers from a frame capture do not exist at runtime, so targets are matched by
   signature: format, and size relative to the presented image. */

#ifndef RSF_D3D11_OBSERVER_H
#define RSF_D3D11_OBSERVER_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OBSERVER_ABI_VERSION 1u

typedef int32_t rsf_observer_result;
#define RSF_OBSERVER_OK ((rsf_observer_result)0)
#define RSF_OBSERVER_ERROR_INVALID_ARGUMENT ((rsf_observer_result)-1)
#define RSF_OBSERVER_ERROR_ABI_MISMATCH ((rsf_observer_result)-2)
#define RSF_OBSERVER_ERROR_ALREADY_INSTALLED ((rsf_observer_result)-3)
#define RSF_OBSERVER_ERROR_NO_DEVICE ((rsf_observer_result)-4)
#define RSF_OBSERVER_ERROR_PATCH_FAILED ((rsf_observer_result)-5)
#define RSF_OBSERVER_ERROR_NOT_READY ((rsf_observer_result)-6)

typedef struct rsf_observer_options {
    uint32_t struct_size;
    uint32_t abi_version;
    /* DXGI_FORMAT to catalogue. Zero catalogues every render target. */
    uint32_t format;
    /* Ignore anything narrower than this, which filters out the small targets that share a
       format with the one being looked for. */
    uint32_t minimum_width;
    /* Upper bound on retained textures, so a misconfigured filter cannot hold the whole frame. */
    uint32_t capacity;
    /* Retain the most recently created constant buffer of exactly this size, so the view uniform
       data can be read back and its layout checked against what engine source predicts. Zero
       disables it. Creation is watched rather than binding for the same reason as textures. */
    uint32_t constant_buffer_bytes;
} rsf_observer_options;

typedef struct rsf_observer_status {
    uint32_t struct_size;
    uint32_t installed;
    uint32_t have_device;
    uint32_t frames_presented;
    uint32_t textures_matched;
    uint32_t textures_created;
    uint32_t present_width;
    uint32_t present_height;
    uint32_t constant_buffers_matched;
} rsf_observer_status;

/* Patch the shared vtables. Safe to call from a worker thread; not from DllMain, because it
   creates a device. */
rsf_observer_result rsf_observer_install(const rsf_observer_options* options);

/* Restore the original vtable entries and release everything retained. */
rsf_observer_result rsf_observer_uninstall(void);

rsf_observer_result rsf_observer_get_status(rsf_observer_status* status);

/* Dump every catalogued texture, writing `<prefix>_<index>.tga` and a matching JSON.
   `view` is an rsf_dump_view from texture_dump.h. */
rsf_observer_result rsf_observer_dump_matches(const char* output_prefix_utf8, uint32_t view,
                                              uint32_t* written);

/* Write the retained constant buffer's bytes to a file, so its layout can be checked against the
   offsets tools/ue4-view-layout.py derives from engine source. Raw bytes only: interpreting them
   is the tool's job, and putting the interpretation here would bake a guess into the runtime. */
rsf_observer_result rsf_observer_dump_constants(const char* output_path_utf8, uint32_t* bytes);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_D3D11_OBSERVER_H */
