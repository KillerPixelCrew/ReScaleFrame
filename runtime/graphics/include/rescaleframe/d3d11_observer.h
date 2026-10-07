/* SPDX-License-Identifier: GPL-3.0-only */
/* Observe D3D11 resource creation through shared device-vtable hooks and source Present through
   detours, with a vtable fallback for Present. Hooks forward the application's original calls.
   Creation callbacks run synchronously under the observer mutex and must only record data: no
   D3D11 calls or observer reentry. Present callbacks run without that mutex and must preserve
   context state. Matching diagnostic textures are retained; retaining identity does not freeze
   their contents. Resource signatures and validation limits are in the graphics README. */

#ifndef RSF_D3D11_OBSERVER_H
#define RSF_D3D11_OBSERVER_H

#include <rescaleframe/texture_dump.h>

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RSF_OBSERVER_ABI_VERSION 8u

/* Reduced vertex-declaration facts; values are D3D11 enums/offsets without a D3D11 header
   dependency. Semantic text is omitted; Unreal's ATTRIBUTE declarations use semantic_index. */
typedef struct rsf_observer_layout_element {
    uint32_t semantic_index;
    uint32_t format;
    uint32_t input_slot;
    uint32_t byte_offset;
    uint32_t per_instance;
} rsf_observer_layout_element;

#define RSF_OBSERVER_MAX_LAYOUT_ELEMENTS 16u

/* A vertex declaration was created. `layout` is the `ID3D11InputLayout*`, borrowed: it is reported
   at the moment it exists and this must not retain it without its own reference.

   `elements` is valid only for the duration of the call, and `count` is the real element count even
   when it exceeds what was copied, so a caller can tell "too many to describe" from "few". */
typedef void (*rsf_observer_layout_fn)(void* user, void* layout,
                                       const rsf_observer_layout_element* elements, uint32_t copied,
                                       uint32_t count);

#define RSF_OBSERVER_STAGE_VERTEX 0u
#define RSF_OBSERVER_STAGE_PIXEL 1u
#define RSF_OBSERVER_STAGE_GEOMETRY 2u
#define RSF_OBSERVER_STAGE_HULL 3u
#define RSF_OBSERVER_STAGE_DOMAIN 4u
#define RSF_OBSERVER_STAGE_COMPUTE 5u

/* Successful shader creation. shader is borrowed; bytecode is caller-owned and valid only for
   this callback. The consumer computes any hash and determines game-specific meaning. */
typedef void (*rsf_observer_shader_fn)(void* user, void* shader, uint32_t stage,
                                       const void* bytecode, uint32_t bytes);

/* A texture was created, with the descriptor it was created from. Reported for every texture, not
   only the ones the format filter retains, because deciding what a texture is for is the caller's
   job and the filter above exists to serve dumping. Borrowed for the call. */
typedef void (*rsf_observer_texture_fn)(void* user, void* texture, uint32_t width, uint32_t height,
                                        uint32_t format, uint32_t mip_levels, uint32_t array_size,
                                        uint32_t sample_count, uint32_t bind_flags,
                                        uint32_t misc_flags);

/* ABI 6: buffer creation, including borrowed initial CPU contents when supplied. This also lets
   capture clients invalidate an old buffer identity before a recycled pointer is used again.
   Called under the observer lock; do not call D3D11 from it. */
typedef void (*rsf_observer_buffer_fn)(void* user, void* buffer, const void* initial,
                                      uint32_t bytes, uint32_t bind_flags);

/* Bind flags, so a caller does not need d3d11.h to read the ones above. These are the D3D11 values. */
#define RSF_OBSERVER_BIND_SHADER_RESOURCE 0x8u
#define RSF_OBSERVER_BIND_RENDER_TARGET 0x20u
#define RSF_OBSERVER_BIND_DEPTH_STENCIL 0x40u
#define RSF_OBSERVER_BIND_UNORDERED_ACCESS 0x80u

/* Before a non-test source D3D11 Present. swapchain is borrowed for this synchronous callback.
   The observer mutex is not held. Run graphics work on this thread and restore affected state. */
typedef void (*rsf_observer_present_fn)(void* user, void* swapchain);
typedef struct rsf_observer_present_event {
    uint32_t struct_size;
    uint32_t completed;
    void* swapchain;
    uint32_t sync_interval;
    uint32_t flags;
    int32_t result; /* HRESULT, valid only when completed is nonzero. */
    uint32_t method; /* 0: Present, 1: Present1. */
} rsf_observer_present_event;
/* Paired around the original Present chain, including test calls. No observer lock is held.
   completed=0 precedes frame/overlay work; completed=1 follows the real call's return. */
typedef void (*rsf_observer_present_event_fn)(void* user, const rsf_observer_present_event* event);


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
    /* Optional creation callbacks on the creating thread, with the observer mutex held. Record
       and return without D3D11 calls or observer reentry. Initial descriptors/blob data are borrowed
       only for the call; retaining a created object requires the consumer's own COM reference. */
    rsf_observer_layout_fn on_layout;
    void* on_layout_user;
    rsf_observer_shader_fn on_shader;
    void* on_shader_user;
    rsf_observer_texture_fn on_texture;
    void* on_texture_user;
    rsf_observer_buffer_fn on_buffer;
    void* on_buffer_user;
    rsf_observer_present_event_fn on_present_event;
    void* on_present_event_user;
} rsf_observer_options;

typedef struct rsf_observer_status {
    uint32_t struct_size;
    uint32_t installed;
    uint32_t have_device;
    /* Non-test Present calls returning S_OK, not physical scanout/generated-frame count. */
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

/* Restore entries/release retained resources after graphics producers are quiescent.
   Returns NOT_READY while a Present callback/forwarding call is still in flight. */
rsf_observer_result rsf_observer_uninstall(void);

/* Acquire the selected ID3D11Device/immediate context with caller-owned references. At least one
   output is required; release every nonnull result. Returns NOT_READY until a device is observed.
   An early creation-hook device can be a helper device; the source presenter later overrides it.
   Acquisition itself does not permit worker-thread context use concurrent with game rendering. */
rsf_observer_result rsf_observer_acquire_device(void** device_out, void** context_out);

/* Fill a mutex-protected diagnostic snapshot; caller initializes status.struct_size. */
rsf_observer_result rsf_observer_get_status(rsf_observer_status* status);
/* Paired around an application-facing D3D11 facade's Present. Updates normal observer
   device/output/counter/dump bookkeeping without invoking callbacks recursively. The facade must
   supply exactly one start followed by one completion, including failed or test presents. */
rsf_observer_result rsf_observer_notify_application_present(const rsf_observer_present_event* event);

/* Copy the UTF-8 prefix and queue diagnostic readback for the next non-test source Present on
   the owning render thread. Last pending request wins; no queue is accumulated. view comes from
   texture_dump.h. Poll dumps_completed; completion counts an attempted batch, not successful
   writes. textures_written/constant_bytes_written describe the last batch. */
rsf_observer_result rsf_observer_request_dump(const char* output_prefix_utf8, uint32_t view);

/* Write the distinct constant buffer sizes seen, with how often each was created. This is how the
   view uniform buffer gets identified when the stock size does not match. */
rsf_observer_result rsf_observer_write_buffer_sizes(const char* output_path_utf8);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* RSF_D3D11_OBSERVER_H */
