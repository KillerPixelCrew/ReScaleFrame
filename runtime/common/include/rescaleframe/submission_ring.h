/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <stdint.h>

/* Private Runtime C interface, linked in-process with its owner. The handle's owner
   keeps it alive through callback retirement. A producer serializes acquire, recording,
   signal publication and advance. */
typedef struct rsf_submission_ring rsf_submission_ring;
#define RSF_SUBMISSION_REMOVED 0u
#define RSF_SUBMISSION_COMPLETE 1u
#define RSF_SUBMISSION_PENDING 2u

#ifdef __cplusplus
extern "C" {
#endif
uint32_t rsf_submission_acquire(const rsf_submission_ring* ring, uint64_t* retired);
uint64_t rsf_submission_pending_value(const rsf_submission_ring* ring);
uint64_t rsf_submission_next_completion(const rsf_submission_ring* ring);
uint64_t rsf_submission_completion(const rsf_submission_ring* ring);
uint64_t rsf_submission_submit(const rsf_submission_ring* ring, uint32_t slot);
void rsf_submission_advance(const rsf_submission_ring* ring, uint32_t slot);
uint32_t rsf_submission_status(const rsf_submission_ring* ring, uint64_t completed);
uint32_t rsf_submission_shared_pair(const rsf_submission_ring* ring, uint64_t* first, uint64_t* second);
uint64_t rsf_submission_shared_tail(const rsf_submission_ring* ring);
void rsf_submission_record_copy(const rsf_submission_ring* ring);
void rsf_submission_copy_submitted(const rsf_submission_ring* ring, uint64_t value);
bool rsf_submission_fence_complete(uint64_t completed, uint64_t required);
#ifdef __cplusplus
}
#endif
