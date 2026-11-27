/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/native_cpu.h>

#define RSF_CPU_SESSION_SERVICE_VERSION 1u

/* Process-lived Runtime endpoints, bound by a host when its graphics session starts.
   A report may pace, so callers must release locks needed by render or Present first.
   execution is optional for graphics sessions that already observe execution internally. */
typedef struct rsf_cpu_session_service {
    uint32_t struct_size, version;
    void (*report)(const rsf_game_cpu_event*);
    int (*ping)(void* window);
    int32_t (*execution)(const rsf_game_render_pass*, uint32_t begin);
} rsf_cpu_session_service;

#ifdef __cplusplus
extern "C" {
#endif
RSF_RUNTIME_API const rsf_cpu_session_service* rsf_fg12_cpu_service(void);
RSF_RUNTIME_API const rsf_cpu_session_service* rsf_latencyvk_cpu_service(void);
#ifdef __cplusplus
}
#endif
