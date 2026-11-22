// SPDX-License-Identifier: MIT
#pragma once
#include <rescaleframe/native_sr_split_vulkan.h>

typedef struct rsf_srvk_session rsf_srvk_session;

#ifdef __cplusplus
extern "C" {
#endif
/* Internal service for engine adapters. The existing setup, handoff, frame
   and fallback records carry their own size and version. A failed open may
   return a cleanup-only owner, subject to the same retirement rules. */
int32_t rsf_srvk_session_open(const rsf_srvk_setup*, const rsf_fsr4vk_handoff*,
    const char* split_runtime_directory, uint32_t preset, rsf_srvk_session**);
int32_t rsf_srvk_session_plan(const rsf_srvk_session*, uint32_t* width, uint32_t* height);
/* A null session recovers a pass refused before evaluation. Pass the refusal
   in provider_result. The effective command buffer comes back in continuation,
   including the null continuation from a terminal split failure. The return
   value describes output availability. provider_result preserves SDK status. */
int32_t rsf_srvk_session_record(rsf_srvk_session*, const rsf_srvk_frame*,
    const rsf_srvk_fallback*, uint32_t split_recording, int32_t* provider_result, void** continuation);
void rsf_srvk_session_retire(rsf_srvk_session*, void* commands);
int32_t rsf_srvk_session_close(rsf_srvk_session*);
#ifdef __cplusplus
}
#endif
