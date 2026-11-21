// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_rdr2_capture_state rsf_rdr2_capture_state;
typedef struct rsf_rdr2_capture_submission rsf_rdr2_capture_submission;
typedef struct rsf_rdr2_capture_progress {
    uint32_t sequence, draws, written;
    uint32_t draw_overflow, timed_out, write_failed, reuse_tail;
    uint32_t requested_input_frames;
} rsf_rdr2_capture_progress;

typedef struct rsf_rdr2_capture_pipeline {
    uint32_t ps, vs;
    int32_t blend;
    uint32_t src, dst, op, src_alpha, dst_alpha, op_alpha, write_mask;
} rsf_rdr2_capture_pipeline;

typedef struct rsf_rdr2_capture_image {
    uint64_t list, generation, submission, queue, fence_value, tick, resource, caller;
    uint32_t sequence, ordinal, width, height, pitch, format, input_index, footprint_format;
    uint32_t state_before, subresource, frame, vertices, instances;
    rsf_rdr2_capture_pipeline pipeline;
} rsf_rdr2_capture_image;

typedef struct rsf_rdr2_capture_draw {
    uint32_t ordinal, frame, vertices, instances;
    uint64_t list, generation, caller, pso;
    rsf_rdr2_capture_pipeline pipeline;
    void* stack[12];
    uint32_t stack_size;
} rsf_rdr2_capture_draw;

typedef struct rsf_rdr2_capture_temporal {
    uint32_t sequence, index, frame, thread, feature_flags, camera_valid;
    uint64_t tick, qpc, list, generation, view, draw_context, job, result;
} rsf_rdr2_capture_temporal;

typedef struct rsf_rdr2_capture_scalar {
    const char* key;
    double value;
} rsf_rdr2_capture_scalar;

typedef struct rsf_rdr2_capture_temporal_view {
    const rsf_rdr2_capture_temporal* facts;
    const rsf_rdr2_capture_scalar* scalars;
    size_t scalar_count, refusal_count;
} rsf_rdr2_capture_temporal_view;

typedef struct rsf_rdr2_capture_presentation {
    uint32_t format, width, height, color_space, color_space_set, metadata_type;
    const uint8_t* metadata;
    size_t metadata_size;
} rsf_rdr2_capture_presentation;

typedef struct rsf_rdr2_capture_bundle {
    const uint16_t* directory;
    size_t directory_len;
    uint64_t base;
    rsf_rdr2_capture_progress progress;
    rsf_rdr2_capture_presentation presentation;
    const rsf_rdr2_capture_draw* draws;
    size_t draw_count, temporal_count;
    uint32_t pid;
} rsf_rdr2_capture_bundle;

#define RSF_RDR2_CAPTURE_SAVE_DRAW 1u
#define RSF_RDR2_CAPTURE_KEEP_TAIL 2u

#ifdef __cplusplus
extern "C" {
#endif
/* The ASI and capture fixture keep the owner for their process lifetime. */
rsf_rdr2_capture_state* rsf_rdr2_capture_create(void);
/* These three entry points may be called without the native graphics mutex. */
void rsf_rdr2_capture_request(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_active(const rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_has_inputs(const rsf_rdr2_capture_state*);
/* Only the diagnostic poll thread samples the key and consumes the request file. */
void rsf_rdr2_capture_poll_request(rsf_rdr2_capture_state*, const uint16_t* request);
/* The caller lends UTF-16 directory units, the role and the current mapping. */
bool rsf_rdr2_capture_write_image(const rsf_rdr2_capture_image*, const uint16_t* directory,
    size_t directory_len, uint32_t pid, const char* role, const uint8_t* data, size_t bytes);
bool rsf_rdr2_capture_write_camera(const uint16_t* directory, size_t directory_len,
    uint32_t pid, uint32_t sequence, uint32_t index, const uint8_t* data, size_t bytes);
/* Called under the capture mutex. All borrowed records and callback results live for the call. */
bool rsf_rdr2_capture_write_bundle(const rsf_rdr2_capture_bundle*,
    void (*read_temporal)(size_t, rsf_rdr2_capture_temporal_view*),
    const char* (*read_refusal)(size_t, size_t), int32_t (*format_scalar)(double, char*, size_t));

/* All operations below run under the native capture mutex, which also orders the graphics
   objects against the capture's admission, drain and publication. */
bool rsf_rdr2_capture_can_arm(const rsf_rdr2_capture_state*, bool readbacks_empty);
void rsf_rdr2_capture_arm(rsf_rdr2_capture_state*, uint64_t tick);
void rsf_rdr2_capture_end_draws(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_timeout(rsf_rdr2_capture_state*, uint64_t tick);
void rsf_rdr2_capture_progress_get(const rsf_rdr2_capture_state*, rsf_rdr2_capture_progress*);
uint32_t rsf_rdr2_capture_commit_draw(rsf_rdr2_capture_state*, uint32_t metadata_count);
uint32_t rsf_rdr2_capture_reserve_input(rsf_rdr2_capture_state*);
void rsf_rdr2_capture_begin_input(rsf_rdr2_capture_state*);
void rsf_rdr2_capture_end_input(rsf_rdr2_capture_state*);
void rsf_rdr2_capture_readback_written(rsf_rdr2_capture_state*, bool success);
void rsf_rdr2_capture_write_failed(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_can_publish(const rsf_rdr2_capture_state*, bool readbacks_empty);
void rsf_rdr2_capture_published(rsf_rdr2_capture_state*);

/* Commands, generations, and state transitions. These calls also use the native capture mutex.
   UINT32_MAX represents a state the probe cannot prove. */
void rsf_rdr2_capture_initial_state(rsf_rdr2_capture_state*, void* resource, uint32_t state);
void rsf_rdr2_capture_transition(rsf_rdr2_capture_state*, void* list, void* resource,
    uint32_t subresource, uint32_t after, bool begin_only);
void rsf_rdr2_capture_alias(rsf_rdr2_capture_state*, void* list, void* resource);
void rsf_rdr2_capture_reset_list(rsf_rdr2_capture_state*, void* list);
uint32_t rsf_rdr2_capture_resource_state(const rsf_rdr2_capture_state*, void* list, void* resource);
uint64_t rsf_rdr2_capture_list_generation(const rsf_rdr2_capture_state*, void* list);
void rsf_rdr2_capture_stamp_copy(rsf_rdr2_capture_state*, void* list, uint64_t* generation, uint64_t* serial);

/* A receipt snapshots the lists and their final recorded states before submission. Native code
   retains only the readback objects named by the receipt, until the original queue call returns.
   The caller frees the receipt after publishing or abandoning it. */
rsf_rdr2_capture_submission* rsf_rdr2_capture_before_submit(rsf_rdr2_capture_state*, uint32_t count, void* const* lists);
bool rsf_rdr2_capture_submission_has_list(const rsf_rdr2_capture_submission*, void* list, uint64_t generation);
uint64_t rsf_rdr2_capture_after_submit(rsf_rdr2_capture_state*, rsf_rdr2_capture_submission*);
void rsf_rdr2_capture_discard_submission(rsf_rdr2_capture_submission*);
#ifdef __cplusplus
}
static_assert(sizeof(rsf_rdr2_capture_progress) == 32);
static_assert(sizeof(rsf_rdr2_capture_pipeline) == 40);
static_assert(sizeof(rsf_rdr2_capture_image) == 160);
static_assert(sizeof(rsf_rdr2_capture_draw) == 192 && offsetof(rsf_rdr2_capture_draw, stack) == 88);
static_assert(sizeof(rsf_rdr2_capture_temporal) == 88);
static_assert(sizeof(rsf_rdr2_capture_scalar) == 16);
static_assert(sizeof(rsf_rdr2_capture_temporal_view) == 32);
static_assert(sizeof(rsf_rdr2_capture_presentation) == 40);
static_assert(sizeof(rsf_rdr2_capture_bundle) == 128 && offsetof(rsf_rdr2_capture_bundle, draws) == 96);
#endif
