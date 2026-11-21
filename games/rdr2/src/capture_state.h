// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_rdr2_capture_state rsf_rdr2_capture_state;
typedef struct rsf_rdr2_capture_submission rsf_rdr2_capture_submission;
typedef struct rsf_rdr2_capture_gpu_retired rsf_rdr2_capture_gpu_retired;
typedef enum rsf_rdr2_capture_hook {
    RSF_RDR2_CAPTURE_CREATE_PSO, RSF_RDR2_CAPTURE_CREATE_LIST, RSF_RDR2_CAPTURE_CREATE_RTV,
    RSF_RDR2_CAPTURE_CREATE_LIBRARY, RSF_RDR2_CAPTURE_LOAD_PSO, RSF_RDR2_CAPTURE_CREATE_STREAM_PSO,
    RSF_RDR2_CAPTURE_LOAD_STREAM_PSO, RSF_RDR2_CAPTURE_RESET, RSF_RDR2_CAPTURE_SET_PSO,
    RSF_RDR2_CAPTURE_SET_RTV, RSF_RDR2_CAPTURE_DRAW, RSF_RDR2_CAPTURE_DRAW_INDEXED,
    RSF_RDR2_CAPTURE_BARRIER, RSF_RDR2_CAPTURE_CREATE_COMMITTED, RSF_RDR2_CAPTURE_CREATE_PLACED,
    RSF_RDR2_CAPTURE_SET_HDR, RSF_RDR2_CAPTURE_HOOK_COUNT
} rsf_rdr2_capture_hook;
typedef struct rsf_rdr2_capture_hook_ops {
    void* (*target)(void* object, uint32_t slot);
    int32_t (*create)(void* target, const void* detour, void** original);
    int32_t (*enable)(void* target);
    void (*report)(uint32_t slot, void* target, int32_t result);
} rsf_rdr2_capture_hook_ops;
typedef struct rsf_rdr2_capture_gpu_ops {
    void (*retain)(void*);
    void (*release)(void*);
    void* (*create_fence)(void* queue);
    bool (*signal)(void* queue, void* fence, uint64_t value);
    void* (*get_buffer)(void* swapchain, uint32_t index);
} rsf_rdr2_capture_gpu_ops;
typedef struct rsf_rdr2_capture_gpu_leases {
    void* buffer;
    void* fence;
    void* queue;
    const rsf_rdr2_capture_gpu_ops* ops;
} rsf_rdr2_capture_gpu_leases;
typedef struct rsf_rdr2_capture_gpu_proof { void* fence; uint64_t value; } rsf_rdr2_capture_gpu_proof;
enum {
    RSF_RDR2_GPU_SIGNAL_ACCEPTED = 0,
    RSF_RDR2_GPU_CREATE_REFUSED = 1,
    RSF_RDR2_GPU_SIGNAL_REFUSED = 2,
    RSF_RDR2_GPU_NO_SLOT = 3
};
typedef struct rsf_rdr2_capture_readback {
    uint64_t generation, submission, fence_value;
    void* list;
    void* queue;
    uint32_t claimed;
} rsf_rdr2_capture_readback;
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

typedef struct rsf_rdr2_capture_draw_binding {
    void* target;
    uint64_t pso;
    rsf_rdr2_capture_pipeline pipeline;
} rsf_rdr2_capture_draw_binding;

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

typedef struct rsf_rdr2_capture_presentation_desc {
    uint32_t format, width, height;
} rsf_rdr2_capture_presentation_desc;

typedef struct rsf_rdr2_capture_input_id {
    uint32_t sequence, index;
} rsf_rdr2_capture_input_id;

typedef struct rsf_rdr2_capture_camera_facts {
    uint32_t frame, thread;
    uint64_t tick, qpc, view, draw_context, job;
} rsf_rdr2_capture_camera_facts;

typedef enum rsf_rdr2_capture_input_role {
    RSF_RDR2_CAPTURE_COLOR, RSF_RDR2_CAPTURE_DEPTH, RSF_RDR2_CAPTURE_MOTION,
    RSF_RDR2_CAPTURE_EXPOSURE, RSF_RDR2_CAPTURE_OUTPUT
} rsf_rdr2_capture_input_role;

#define RSF_RDR2_CAPTURE_CAMERA_BYTES 0x800u

typedef struct rsf_rdr2_capture_bundle {
    const uint16_t* directory;
    size_t directory_len;
    uint64_t base;
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
/* The native parser lends shader bytes through the hash call, before taking the capture mutex. */
bool rsf_rdr2_capture_hash_shader(const uint8_t* bytes, size_t length, uint32_t* out);
/* Only the diagnostic poll thread samples the key and consumes the request file. */
void rsf_rdr2_capture_poll_request(rsf_rdr2_capture_state*, const uint16_t* request);
/* The caller lends UTF-16 directory units, the role and the current mapping. */
bool rsf_rdr2_capture_write_image(const rsf_rdr2_capture_image*, const uint16_t* directory,
    size_t directory_len, uint32_t pid, const char* role, const uint8_t* data, size_t bytes);
/* Every NGX invocation reports both phases on the calling thread, including skipped ones. */
bool rsf_rdr2_capture_track_evaluation(bool ending);
const char* rsf_rdr2_capture_input_scalar_key(uint32_t index, bool* floating);

/* All operations below run under the native capture mutex, which also orders the graphics
   objects against the capture's admission, drain and publication. */
bool rsf_rdr2_capture_can_arm(const rsf_rdr2_capture_state*, bool readbacks_empty);
void rsf_rdr2_capture_arm(rsf_rdr2_capture_state*, uint64_t tick, void* swapchain_identity,
    const rsf_rdr2_capture_presentation_desc*);
void rsf_rdr2_capture_end_draws(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_timeout(rsf_rdr2_capture_state*, uint64_t tick);
void rsf_rdr2_capture_progress_get(const rsf_rdr2_capture_state*, rsf_rdr2_capture_progress*);
uint32_t rsf_rdr2_capture_commit_draw(rsf_rdr2_capture_state*);
void rsf_rdr2_capture_readback_written(rsf_rdr2_capture_state*, bool success);
bool rsf_rdr2_capture_can_publish(const rsf_rdr2_capture_state*, bool readbacks_empty);
void rsf_rdr2_capture_published(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_write_bundle(rsf_rdr2_capture_state*, const rsf_rdr2_capture_bundle*,
    int32_t (*format_scalar)(double, char*, size_t), size_t* draw_count, size_t* temporal_count);

/* Swapchain observations use canonical IUnknown identity. Any lent bytes are copied. */
void rsf_rdr2_capture_reset_presentation(rsf_rdr2_capture_state*, void* identity);
void rsf_rdr2_capture_record_color_space(rsf_rdr2_capture_state*, void* identity, uint32_t color_space);
void rsf_rdr2_capture_record_hdr(rsf_rdr2_capture_state*, void* identity, uint32_t type,
    const uint8_t* metadata, size_t size);
/* Valid flags are copied, including zero. Null clears any previous feature at this handle. */
void rsf_rdr2_capture_record_feature(rsf_rdr2_capture_state*, void* handle, const uint32_t* flags);
bool rsf_rdr2_capture_begin_input(rsf_rdr2_capture_state*, void* list, void* handle, rsf_rdr2_capture_input_id*);
bool rsf_rdr2_capture_commit_input(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_input_result(rsf_rdr2_capture_state*, void* list, uint64_t result, rsf_rdr2_capture_input_id*);
void rsf_rdr2_capture_finish_input(rsf_rdr2_capture_state*);
void rsf_rdr2_capture_input_camera(rsf_rdr2_capture_state*, const rsf_rdr2_capture_input_id*,
    const rsf_rdr2_capture_camera_facts*, const uint8_t* camera_or_null);
void rsf_rdr2_capture_input_scalar(rsf_rdr2_capture_state*, const rsf_rdr2_capture_input_id*,
    uint32_t index, double value, bool available);
void rsf_rdr2_capture_input_resource_refused(rsf_rdr2_capture_state*, const rsf_rdr2_capture_input_id*,
    uint32_t role, bool missing);

/* Observation identities are borrowed from the native callbacks and hold no COM reference.
   Null pipeline facts discard an older record. A null handle clears the bound target. */
void rsf_rdr2_capture_record_pipeline(rsf_rdr2_capture_state*, void* pso, const rsf_rdr2_capture_pipeline*);
void rsf_rdr2_capture_record_rtv(rsf_rdr2_capture_state*, uintptr_t handle, void* resource);
void rsf_rdr2_capture_bind_rtv(rsf_rdr2_capture_state*, void* list, const uintptr_t* first_handle);
void rsf_rdr2_capture_bind_pipeline(rsf_rdr2_capture_state*, void* list, void* pso);
bool rsf_rdr2_capture_get_draw_binding(const rsf_rdr2_capture_state*, void* list, rsf_rdr2_capture_draw_binding*);
bool rsf_rdr2_capture_record_draw(rsf_rdr2_capture_state*, const rsf_rdr2_capture_draw*);

/* Commands, generations, and state transitions. These calls also use the native capture mutex.
   UINT32_MAX represents a state the probe cannot prove. */
void rsf_rdr2_capture_initial_state(rsf_rdr2_capture_state*, void* resource, uint32_t state);
void rsf_rdr2_capture_transition(rsf_rdr2_capture_state*, void* list, void* resource,
    uint32_t subresource, uint32_t after, bool begin_only);
void rsf_rdr2_capture_alias(rsf_rdr2_capture_state*, void* list, void* resource);
void rsf_rdr2_capture_reset_list(rsf_rdr2_capture_state*, void* list, void* pso);
uint32_t rsf_rdr2_capture_resource_state(const rsf_rdr2_capture_state*, void* list, void* resource);
void rsf_rdr2_capture_stamp_copy(rsf_rdr2_capture_state*, void* list, rsf_rdr2_capture_readback*);
bool rsf_rdr2_capture_reuse_readback(const rsf_rdr2_capture_state*, const rsf_rdr2_capture_readback*,
    void* list, uint64_t completed);
bool rsf_rdr2_capture_readback_complete(const rsf_rdr2_capture_readback*, uint64_t completed);

/* A receipt snapshots the lists and their final recorded states before submission. Native code
   retains only the readback objects named by the receipt, until the original queue call returns.
   The caller frees the receipt after publishing or abandoning it. */
rsf_rdr2_capture_submission* rsf_rdr2_capture_before_submit(rsf_rdr2_capture_state*, uint32_t count, void* const* lists);
bool rsf_rdr2_capture_claim_readback(const rsf_rdr2_capture_submission*, rsf_rdr2_capture_readback*);
void rsf_rdr2_capture_submit_readback(rsf_rdr2_capture_readback*, void* queue, uint64_t submission);
bool rsf_rdr2_capture_readback_awaits_fence(const rsf_rdr2_capture_readback*, void* queue);
bool rsf_rdr2_capture_fence_readback(rsf_rdr2_capture_readback*, void* queue, uint64_t value);
uint64_t rsf_rdr2_capture_after_submit(rsf_rdr2_capture_state*, rsf_rdr2_capture_submission*);
void rsf_rdr2_capture_discard_submission(rsf_rdr2_capture_submission*);
void rsf_rdr2_capture_bind_hook(const rsf_rdr2_capture_hook_ops*, uint32_t role, void* object, const void* detour);
uintptr_t rsf_rdr2_capture_hook_original(uint32_t role);
bool rsf_rdr2_capture_gpu_adopt(rsf_rdr2_capture_gpu_leases*, void* buffer, const rsf_rdr2_capture_gpu_ops*);
void rsf_rdr2_capture_gpu_bind_queue(rsf_rdr2_capture_gpu_leases*, void* queue);
void rsf_rdr2_capture_gpu_bind_fence(rsf_rdr2_capture_gpu_leases*, void* fence);
void rsf_rdr2_capture_gpu_drop(rsf_rdr2_capture_gpu_leases*);
uint32_t rsf_rdr2_capture_gpu_signal(rsf_rdr2_capture_state*, void* queue,
    const rsf_rdr2_capture_gpu_ops*, rsf_rdr2_capture_gpu_proof*);
bool rsf_rdr2_capture_gpu_arm_backbuffers(rsf_rdr2_capture_state*, void* swapchain, uint32_t count,
    const rsf_rdr2_capture_gpu_ops*);
bool rsf_rdr2_capture_gpu_accepts_target(const rsf_rdr2_capture_state*, void* target);
void rsf_rdr2_capture_gpu_select_target(rsf_rdr2_capture_state*, void* target);
void* rsf_rdr2_capture_gpu_selected(const rsf_rdr2_capture_state*, void* swapchain);
rsf_rdr2_capture_gpu_retired* rsf_rdr2_capture_gpu_take_idle(rsf_rdr2_capture_state*);
/* Drop detached queue and backbuffer owners outside the capture mutex. */
void rsf_rdr2_capture_gpu_drop_idle(rsf_rdr2_capture_gpu_retired*);
#ifdef __cplusplus
}
static_assert(sizeof(rsf_rdr2_capture_progress) == 32);
static_assert(sizeof(rsf_rdr2_capture_readback) == 48 && offsetof(rsf_rdr2_capture_readback, list) == 24);
static_assert(sizeof(rsf_rdr2_capture_gpu_ops) == 40 && sizeof(rsf_rdr2_capture_gpu_leases) == 32);
static_assert(sizeof(rsf_rdr2_capture_gpu_proof) == 16);
static_assert(RSF_RDR2_CAPTURE_HOOK_COUNT == 16 && sizeof(rsf_rdr2_capture_hook_ops) == 32);
static_assert(sizeof(rsf_rdr2_capture_pipeline) == 40);
static_assert(sizeof(rsf_rdr2_capture_draw_binding) == 56 && offsetof(rsf_rdr2_capture_draw_binding, pipeline) == 16);
static_assert(sizeof(rsf_rdr2_capture_image) == 160);
static_assert(sizeof(rsf_rdr2_capture_draw) == 192 && offsetof(rsf_rdr2_capture_draw, stack) == 88);
static_assert(sizeof(rsf_rdr2_capture_presentation_desc) == 12);
static_assert(sizeof(rsf_rdr2_capture_input_id) == 8 && RSF_RDR2_CAPTURE_OUTPUT == 4);
static_assert(sizeof(rsf_rdr2_capture_camera_facts) == 48 && offsetof(rsf_rdr2_capture_camera_facts, tick) == 8);
static_assert(sizeof(rsf_rdr2_capture_bundle) == 32 && offsetof(rsf_rdr2_capture_bundle, pid) == 24);
#endif
