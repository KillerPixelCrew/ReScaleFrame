/* SPDX-License-Identifier: MIT */
#pragma once
#include "sharpening_vulkan_core.h"

/* A recording host serializes calls, retires completed and reset command buffers,
   and drains all users before clearing or destroying this state. */
typedef struct rsf_sharpenvk_host {
    rsf_sharpenvk* owner;
    uint32_t failure_logged;
} rsf_sharpenvk_host;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_sharpenvk_host_destroy(rsf_sharpenvk_host*);
void rsf_sharpenvk_host_clear(rsf_sharpenvk_host*);
void rsf_sharpenvk_host_retire(rsf_sharpenvk_host*, void* commands);
uint64_t rsf_sharpenvk_host_dispatches(const rsf_sharpenvk_host*);
uint32_t rsf_sharpenvk_host_run(rsf_sharpenvk_host*, const rsf_srvk_setup*,
    void* commands, const rsf_srvk_frame*, uint64_t output);
#ifdef __cplusplus
}
#endif
