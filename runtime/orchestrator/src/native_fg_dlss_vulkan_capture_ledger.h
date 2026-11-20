// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_fg_dlss_vulkan.h>

typedef struct rsf_dlss_capture_ledger rsf_dlss_capture_ledger;

typedef struct rsf_dlss_capture_source {
    rsf_frame_record record;
    rsf_dlssgvk_status sdk;
    uint32_t generated_count, configured;
} rsf_dlss_capture_source;

#ifdef __cplusplus
extern "C" {
#endif
/* Private state. The native capture owner serializes calls and unregisters
   the SDK callback before destroying the ledger. No graphics handles cross. */
rsf_dlss_capture_ledger* rsf_core_dlss_capture_create(void);
void rsf_core_dlss_capture_destroy(rsf_dlss_capture_ledger*);
/* Null source clears the observation anchor. Codes 1 and 2 report an
   ambiguous identity or a source that cannot fit in the SDK token domain. */
uint32_t rsf_core_dlss_capture_remember(rsf_dlss_capture_ledger*, const rsf_dlss_capture_source*);
uint32_t rsf_core_dlss_capture_source(const rsf_dlss_capture_ledger*, uint64_t sdk_frame,
    rsf_dlss_capture_source*);
uint32_t rsf_core_dlss_capture_observed(const rsf_dlss_capture_ledger*, rsf_dlss_capture_source*);
uint32_t rsf_core_dlss_capture_pending(const rsf_dlss_capture_ledger*, uint32_t queue_family);
void rsf_core_dlss_capture_submitted(rsf_dlss_capture_ledger*, uint32_t queue_family);
void rsf_core_dlss_capture_completed(rsf_dlss_capture_ledger*, uint32_t queue_family);
void rsf_core_dlss_capture_unsubmitted(rsf_dlss_capture_ledger*);
uint32_t rsf_core_dlss_capture_can_record(const rsf_dlss_capture_ledger*);
uint32_t rsf_core_dlss_capture_can_flush(const rsf_dlss_capture_ledger*);
/* The caller has already drained the presenter and device. This proves
   completion of submitted copies only. */
void rsf_core_dlss_capture_drained(rsf_dlss_capture_ledger*);
uint64_t rsf_core_dlss_capture_sequence(rsf_dlss_capture_ledger*);
#ifdef __cplusplus
}
#endif
