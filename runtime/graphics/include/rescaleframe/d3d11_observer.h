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

#include <rescaleframe/texture_dump.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OBSERVER_ABI_VERSION 4u

/* Called on the game's render thread, immediately before its own Present.

   That is the one place in a frame where the finished image exists and nothing has been shown yet,
   which is what anything drawing over the game needs. `swapchain` is an `IDXGISwapChain*`, borrowed
   for the duration of the call. Anything this does to the device context it must put back: the game
   is between its own draws and did not ask for its state to change. */
typedef void (*rsf_observer_present_fn)(void* user, void* swapchain);

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
    /* Retain the most recent constant buffer of each distinct size in this range, so the view
       uniform data can be found and its layout checked against what engine source predicts.
       A zero maximum disables it. A range rather than one size because the engine is a vendor
       branch and the stock size is only a starting guess. */
    uint32_t constant_buffer_min_bytes;
    uint32_t constant_buffer_max_bytes;
    /* Where a dump reports its progress. Every step inside a present is announced before it runs,
       so a dump that takes the process down leaves the resource and the step it reached on disk.
       Optional, and passed on to every texture dump the observer performs. */
    rsf_dump_log_fn log;
    void* log_user;
    /* Also run the motion decode pass over each dumped target and write the result beside it.
       This is how the pass every backend depends on gets checked against the game's own buffer
       rather than only against values a test made up. Zero disables it. */
    uint32_t decode_motion;
    /* The game's encoding, as `(stored - bias) * scale`, and the value written where the source
       held its clear value. Passed in rather than assumed, because the encoding belongs to the
       game and this code does not know which game it is in. */
    float motion_scale;
    float motion_bias;
    float motion_invalid_value;
    /* Optional. Invoked before every Present, which is where an overlay draws. */
    rsf_observer_present_fn on_present;
    void* on_present_user;
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
    uint32_t distinct_buffer_sizes;
    /* Increments each time a requested dump finishes inside a present. */
    uint32_t dumps_completed;
    uint32_t textures_written;
    uint32_t constant_bytes_written;
} rsf_observer_status;

/* Patch the shared vtables. Safe to call from a worker thread; not from DllMain, because it
   creates a device. */
rsf_observer_result rsf_observer_install(const rsf_observer_options* options);

/* Restore the original vtable entries and release everything retained. */
rsf_observer_result rsf_observer_uninstall(void);

/* Hand out the device the observer found, and its immediate context.

   Anything that wants to do graphics work inside this game needs a device, and creating one of its
   own would be a second device: resources could not be shared with the game's, which is the whole
   point. The observer already has the game's, because it watched it being used.

   Both are AddRef'd and the caller releases them. Either pointer may be null if it is not wanted.
   Returns RSF_OBSERVER_ERROR_NOT_READY before the game has created anything, which is most of
   startup, so a caller has to be prepared to ask again rather than give up. */
rsf_observer_result rsf_observer_acquire_device(void** device_out, void** context_out);

rsf_observer_result rsf_observer_get_status(rsf_observer_status* status);

/* Ask for a dump to be taken. The work happens inside the next present, on whichever thread the
   game renders from.

   A device context may not be used from two threads at once, so reading a resource from a worker
   thread races the game's own rendering: it returns whatever the staging copy happened to hold
   and can take the process down. Presenting is the one moment we are already on the right thread
   at a defined point in the frame.

   `view` is an rsf_dump_view from texture_dump.h. Returns immediately; poll
   rsf_observer_get_status for `dumps_completed` to know when it is done. */
rsf_observer_result rsf_observer_request_dump(const char* output_prefix_utf8, uint32_t view);

/* Write the distinct constant buffer sizes seen, with how often each was created. This is how the
   view uniform buffer gets identified when the stock size does not match. */
rsf_observer_result rsf_observer_write_buffer_sizes(const char* output_path_utf8);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_D3D11_OBSERVER_H */
