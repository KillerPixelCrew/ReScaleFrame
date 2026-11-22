// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct rsf_rdr2_capture_state rsf_rdr2_capture_state;
typedef enum rsf_rdr2_capture_hook {
    RSF_RDR2_CAPTURE_CREATE_PSO, RSF_RDR2_CAPTURE_CREATE_LIST, RSF_RDR2_CAPTURE_CREATE_RTV,
    RSF_RDR2_CAPTURE_CREATE_LIBRARY, RSF_RDR2_CAPTURE_LOAD_PSO, RSF_RDR2_CAPTURE_CREATE_STREAM_PSO,
    RSF_RDR2_CAPTURE_LOAD_STREAM_PSO, RSF_RDR2_CAPTURE_RESET, RSF_RDR2_CAPTURE_SET_PSO,
    RSF_RDR2_CAPTURE_SET_RTV, RSF_RDR2_CAPTURE_DRAW, RSF_RDR2_CAPTURE_DRAW_INDEXED,
    RSF_RDR2_CAPTURE_BARRIER, RSF_RDR2_CAPTURE_CREATE_COMMITTED, RSF_RDR2_CAPTURE_CREATE_PLACED,
    RSF_RDR2_CAPTURE_SET_HDR, RSF_RDR2_CAPTURE_HOOK_COUNT
} rsf_rdr2_capture_hook;
enum { RSF_RDR2_CAPTURE_UNOBSERVED_HOOK_TARGET = -2 };
typedef struct rsf_rdr2_capture_hook_ops {
    void* (*target)(void* object, uint32_t slot);
    int32_t (*create)(void* target, const void* detour, void** original);
    int32_t (*enable)(void* target);
    void (*report)(uint32_t slot, void* target, int32_t result);
} rsf_rdr2_capture_hook_ops;
typedef struct rsf_rdr2_capture_gpu_ops rsf_rdr2_capture_gpu_ops;
typedef struct rsf_rdr2_capture_gpu_leases rsf_rdr2_capture_gpu_leases;
enum {
    RSF_RDR2_GPU_SIGNAL_ACCEPTED = 0,
    RSF_RDR2_GPU_CREATE_REFUSED = 1,
    RSF_RDR2_GPU_SIGNAL_REFUSED = 2,
    RSF_RDR2_GPU_NO_SLOT = 3,
    RSF_RDR2_GPU_NO_SIGNAL_NEEDED = 4,
    RSF_RDR2_GPU_UNPAIRED_SUBMISSION = 5
};
typedef struct rsf_rdr2_capture_progress {
    uint32_t sequence, draws, written;
    uint32_t draw_overflow, timed_out, write_failed;
    uint32_t requested_input_frames, readbacks_unavailable;
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
    bool pipeline_known;
} rsf_rdr2_capture_draw_binding;

typedef struct rsf_rdr2_capture_image {
    uint64_t list, generation, submission, queue, fence_value, tick, resource, caller;
    uint32_t sequence, ordinal, width, height, pitch, format, input_index, footprint_format;
    uint32_t state_before, subresource, frame, vertices, instances;
    rsf_rdr2_capture_pipeline pipeline;
    bool pipeline_known;
    uint64_t pso;
} rsf_rdr2_capture_image;

typedef bool (*rsf_rdr2_capture_copy)(void* list, void* target, uint32_t before,
    rsf_rdr2_capture_gpu_leases*, rsf_rdr2_capture_image*);
typedef uint64_t (*rsf_rdr2_capture_completed)(void* fence);
typedef bool (*rsf_rdr2_capture_removed)(void* buffer);
typedef bool (*rsf_rdr2_capture_write)(const rsf_rdr2_capture_gpu_leases*, const rsf_rdr2_capture_image*, const char* role);

typedef struct rsf_rdr2_capture_draw {
    uint32_t ordinal, frame, vertices, instances;
    uint64_t list, generation, caller, pso;
    rsf_rdr2_capture_pipeline pipeline;
    void* stack[12];
    uint32_t stack_size;
    bool pipeline_known;
} rsf_rdr2_capture_draw;

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

typedef struct rsf_rdr2_capture_poll_ops {
    void (*lock)(void* context);
    void (*unlock)(void* context);
    rsf_rdr2_capture_completed completed;
    rsf_rdr2_capture_removed removed;
    rsf_rdr2_capture_write write;
    void (*timeout)(uint32_t sequence);
    int32_t (*format_scalar)(double, char*, size_t);
    void (*report)(uint32_t sequence, bool saved, size_t inputs, size_t draws, uint32_t written);
} rsf_rdr2_capture_poll_ops;

enum {
    RSF_RDR2_CAPTURE_COPY_REFUSED = 0,
    RSF_RDR2_CAPTURE_COPY_RECORDED = 1,
    RSF_RDR2_CAPTURE_COPY_NO_SLOT = 2,
    RSF_RDR2_CAPTURE_DRAW_SAVE_EVENT = 3
};

#ifdef __cplusplus
extern "C" {
#endif
/* The ASI and capture fixture keep the owner for their process lifetime. */
rsf_rdr2_capture_state* rsf_rdr2_capture_create(void);
/* These three entry points may be called without the native graphics mutex. */
void rsf_rdr2_capture_request(rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_active(const rsf_rdr2_capture_state*);
bool rsf_rdr2_capture_has_inputs(const rsf_rdr2_capture_state*);
/* Native PSO calls lend their accepted descriptors before taking the capture mutex. */
bool rsf_rdr2_capture_graphics_facts(const void* descriptor, rsf_rdr2_capture_pipeline*);
bool rsf_rdr2_capture_stream_facts(const void* descriptor, rsf_rdr2_capture_pipeline*);
/* The poll worker owns the directory and the detached batch. Native leaves borrow both. */
void rsf_rdr2_capture_poll(rsf_rdr2_capture_state*, void* context, const rsf_rdr2_capture_poll_ops*,
    const uint16_t* request, const uint16_t* directory, size_t directory_len, uint64_t base, bool fixture);
const rsf_rdr2_capture_gpu_ops* rsf_rdr2_capture_d3d12_ops(void);
uint64_t rsf_rdr2_capture_d3d12_completed(void* fence);
bool rsf_rdr2_capture_d3d12_removed(void* buffer);
bool rsf_rdr2_capture_d3d12_copy(void* list, void* target, uint32_t before,
    rsf_rdr2_capture_gpu_leases*, rsf_rdr2_capture_image*, int32_t* allocation_hresult);
bool rsf_rdr2_capture_d3d12_write(const rsf_rdr2_capture_gpu_leases*, const rsf_rdr2_capture_image*,
    const uint16_t* directory, size_t directory_len, uint32_t pid, const char* role, int32_t* map_hresult);
bool rsf_rdr2_capture_d3d12_present(rsf_rdr2_capture_state*, void* verified_swapchain, void** completed_target);
/* Every NGX invocation reports both phases on the calling thread, including skipped ones. */
bool rsf_rdr2_capture_track_evaluation(bool ending);

/* All operations below run under the native capture mutex, which also orders the graphics
   objects against the capture's admission, drain and publication. */
void rsf_rdr2_capture_progress_get(const rsf_rdr2_capture_state*, rsf_rdr2_capture_progress*);
size_t rsf_rdr2_capture_pending_count(const rsf_rdr2_capture_state*);

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
void rsf_rdr2_capture_input_scalars(rsf_rdr2_capture_state*, const rsf_rdr2_capture_input_id*, const void* parameters,
    bool (*read)(const void*, const char* key, bool floating, double* value));
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

/* The copy leaf borrows the shot only during the call and returns true after recording it.
   The draw call returns DRAW_SAVE_EVENT when the render thread should sample the draw stack. */
uint32_t rsf_rdr2_capture_input_shot(rsf_rdr2_capture_state*, const rsf_rdr2_capture_input_id*,
    uint32_t role, void* list, void* resource, uint32_t before, rsf_rdr2_capture_copy);
uint32_t rsf_rdr2_capture_draw_shot(rsf_rdr2_capture_state*, void* list, void* resource, uint32_t before,
    rsf_rdr2_capture_copy, rsf_rdr2_capture_completed, rsf_rdr2_capture_draw*);

/* Call both phases on the submitting thread. Rust keeps the exact generation receipt
   until the original queue call returns. */
void rsf_rdr2_capture_before_submit(rsf_rdr2_capture_state*, void* queue, uint32_t count, void* const* lists);
uint32_t rsf_rdr2_capture_after_submit(rsf_rdr2_capture_state*, void* queue, const rsf_rdr2_capture_gpu_ops*);
void rsf_rdr2_capture_bind_hook(const rsf_rdr2_capture_hook_ops*, uint32_t role, void* object, const void* detour);
uintptr_t rsf_rdr2_capture_hook_original(uint32_t role);
bool rsf_rdr2_capture_gpu_accepts_target(const rsf_rdr2_capture_state*, void* target);
#ifdef __cplusplus
}
static_assert(sizeof(rsf_rdr2_capture_progress) == 32);
static_assert(RSF_RDR2_CAPTURE_HOOK_COUNT == 16 && sizeof(rsf_rdr2_capture_hook_ops) == 32);
static_assert(sizeof(rsf_rdr2_capture_pipeline) == 40);
static_assert(sizeof(rsf_rdr2_capture_draw_binding) == 64 && offsetof(rsf_rdr2_capture_draw_binding, pipeline_known) == 56);
static_assert(sizeof(rsf_rdr2_capture_image) == 168 && offsetof(rsf_rdr2_capture_image, pipeline) == 116 &&
    offsetof(rsf_rdr2_capture_image, pso) == 160);
static_assert(sizeof(rsf_rdr2_capture_draw) == 192 && offsetof(rsf_rdr2_capture_draw, stack) == 88);
static_assert(sizeof(rsf_rdr2_capture_input_id) == 8 && RSF_RDR2_CAPTURE_OUTPUT == 4);
static_assert(sizeof(rsf_rdr2_capture_camera_facts) == 48 && offsetof(rsf_rdr2_capture_camera_facts, tick) == 8);
static_assert(sizeof(rsf_rdr2_capture_poll_ops) == 64 && offsetof(rsf_rdr2_capture_poll_ops, report) == 56);
#endif
