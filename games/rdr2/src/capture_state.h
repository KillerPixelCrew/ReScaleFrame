// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stdint.h>

typedef struct rsf_rdr2_capture_state rsf_rdr2_capture_state;
typedef struct rsf_rdr2_capture_progress {
    uint32_t sequence, draws, written;
    uint32_t draw_overflow, timed_out, write_failed, reuse_tail;
    uint32_t requested_input_frames;
} rsf_rdr2_capture_progress;

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
#ifdef __cplusplus
}
static_assert(sizeof(rsf_rdr2_capture_progress) == 32);
#endif
