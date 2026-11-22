// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_frame.h>

#define RSF_FGVK_FRAME_SLOTS 6u
typedef struct rsf_fgvk_frame_slot {
    rsf_frame_record record;
    uint64_t recording_sequence;
    void* prepare_commands;
    void* hudless_commands;
    uint32_t guides, prepared, hudless, submitted_prepare, submitted_hudless;
    uint32_t recorded_prepare, recorded_hudless;
    uint32_t source_pending, consumer_pending, completion_unknown;
} rsf_fgvk_frame_slot;

typedef struct rsf_fgvk_timeline {
    rsf_frame_record last_prepared;
    uint64_t recording_sequence, presented_sequence;
} rsf_fgvk_timeline;

typedef struct rsf_fgvk_submit_plan {
    uint32_t fences, discarded_prepare, discarded_hudless;
} rsf_fgvk_submit_plan;

#ifdef __cplusplus
extern "C" {
#endif
/* The native owner serializes all calls and supplies actual command and fence
   results. The six state pointers remain tied to their native resource slots. */
uint32_t rsf_core_fgvk_unsubmitted(const rsf_fgvk_frame_slot*);
uint32_t rsf_core_fgvk_can_prepare(const rsf_fgvk_frame_slot*);
void rsf_core_fgvk_prepare_begin(rsf_fgvk_timeline*, rsf_fgvk_frame_slot*, const rsf_frame_record*, void* commands);
void rsf_core_fgvk_prepare_accepted(rsf_fgvk_timeline*, rsf_fgvk_frame_slot*);
uint32_t rsf_core_fgvk_hudless_begin(rsf_fgvk_frame_slot*, const rsf_frame_record*, void* commands);
/* Bit i refers to native slot i. Duplicate list notifications are idempotent.
   A reused pointer belongs to its newest recording. Discarded recordings can
   no longer establish their images' recorded layouts. The array contains six
   distinct writable slots. */
void rsf_core_fgvk_submitted(rsf_fgvk_frame_slot* const slots[RSF_FGVK_FRAME_SLOTS],
    void* const* commands, uint32_t count, rsf_fgvk_submit_plan* out);
uint32_t rsf_core_fgvk_present_candidate(const rsf_fgvk_timeline*, const rsf_fgvk_frame_slot*,
    uint64_t source_frame, uint32_t identity_matches, uint32_t requested);
void rsf_core_fgvk_presented(rsf_fgvk_timeline*, const rsf_fgvk_frame_slot*, uint64_t source_frame,
    uint32_t identity_matches);
#ifdef __cplusplus
}
#endif
