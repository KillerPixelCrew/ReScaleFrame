// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/game_frame.h>

#define RSF_DLSS_VK_FRAME_SLOTS 6u
typedef struct rsf_dlss_vk_slot {
    rsf_frame_record record;
    uint64_t recording_sequence;
    void* prepare_commands;
    void* hudless_commands;
    uint32_t prepared, hudless, submitted_prepare, submitted_hudless;
    uint32_t recorded_prepare, recorded_hudless;
    uint32_t source_pending, consumer_pending, completion_unknown, sdk_inputs_pending;
} rsf_dlss_vk_slot;

typedef struct rsf_dlss_vk_timeline {
    rsf_frame_record last_prepared;
    uint64_t recording_sequence, presented_sequence;
} rsf_dlss_vk_timeline;

#ifdef __cplusplus
extern "C" {
#endif
/* The native owner serializes all calls and supplies actual command and fence
   results. The six state pointers remain tied to their native resource slots. */
uint32_t rsf_core_dlss_vk_unsubmitted(const rsf_dlss_vk_slot*);
uint32_t rsf_core_dlss_vk_can_prepare(const rsf_dlss_vk_slot*);
void rsf_core_dlss_vk_prepare_begin(rsf_dlss_vk_timeline*, rsf_dlss_vk_slot*, const rsf_frame_record*, void* commands);
void rsf_core_dlss_vk_prepare_accepted(rsf_dlss_vk_timeline*, const rsf_dlss_vk_slot*);
uint32_t rsf_core_dlss_vk_hudless_begin(rsf_dlss_vk_slot*, const rsf_frame_record*, void* commands);
/* Bit i requests a source completion fence for native slot i. Duplicate list
   notifications are idempotent, and a reused pointer belongs to its newest recording. */
uint32_t rsf_core_dlss_vk_submitted(rsf_dlss_vk_slot* const slots[RSF_DLSS_VK_FRAME_SLOTS],
    void* const* commands, uint32_t count);
uint32_t rsf_core_dlss_vk_present_candidate(const rsf_dlss_vk_timeline*, const rsf_dlss_vk_slot*,
    uint64_t source_frame, uint32_t identity_matches, uint32_t requested);
void rsf_core_dlss_vk_presented(rsf_dlss_vk_timeline*, const rsf_dlss_vk_slot*, uint64_t source_frame,
    uint32_t identity_matches);
#ifdef __cplusplus
}
#endif
