/* SPDX-License-Identifier: GPL-3.0-only */
#pragma once
#include <rescaleframe/input_dump.h>
typedef struct rsf_input_dump12 rsf_input_dump12;
#ifdef __cplusplus
extern "C" {
#endif
RSF_FRAME_DUMP_API int32_t rsf_input_dump12_create(rsf_input_dump12**);
/* The caller supplies the actual state at a leased, unpredicated command boundary.
   The reader retains source/readback COM references through completion; placed-resource
   backing heaps and engine pool ownership must still cover submitted GPU use. */
RSF_FRAME_DUMP_API int32_t rsf_input_dump12_record(rsf_input_dump12*, void* command_list, void* texture,
    const rsf_frame_record*, const char* name, uint32_t state, uint32_t slot, rsf_dump_view view, float scale);
/* After the command lists have really been submitted on this serialized queue. */
RSF_FRAME_DUMP_API int32_t rsf_input_dump12_submitted(rsf_input_dump12*, void* queue, uint32_t slot);
RSF_FRAME_DUMP_API int32_t rsf_input_dump12_poll(rsf_input_dump12*);
/* Only after the caller has discarded this slot's unsubmitted command recordings. */
RSF_FRAME_DUMP_API void rsf_input_dump12_abandon(rsf_input_dump12*, uint32_t slot);
/* NOT_READY preserves the reader while any submitted/unsubmitted copy remains. */
RSF_FRAME_DUMP_API int32_t rsf_input_dump12_destroy(rsf_input_dump12*);
#ifdef __cplusplus
}
#endif
