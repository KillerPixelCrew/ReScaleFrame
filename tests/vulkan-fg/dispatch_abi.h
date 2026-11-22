/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdbool.h>
#include <rescaleframe/native_fg_vulkan.h>
#include "results.h"
#include "../../runtime/vulkan/fsr4-fg/bridge_api.h"
#include "../../runtime/orchestrator/src/native_latency_vulkan_presentation.h"

typedef struct rsf_dispatch_fixture_state {
    uint32_t ffx_creates, dlss_creates, destroys, enabled, generated_count, calls, explicit_presents;
    bool refuse_destroy, refuse_create, refuse_state, missing_owner, fail_after_create, fail_drain;
    const rsf_fgvk_setup* borrowed_setup;
    const rsf_frame_record* presented_source;
    uint32_t fsr4_creates, xess_creates;
    int32_t fsr4_create_result;
    bool fsr4_partial, refuse_latency;
    rsf_fgvk_latency_status xell;
} rsf_dispatch_fixture_state;

#ifdef __cplusplus
extern "C" {
#endif
rsf_dispatch_fixture_state* rsf_fixture_dispatch_state(void);
void rsf_fixture_block_sleep(void);
bool rsf_fixture_wait_sleep(void);
void rsf_fixture_release_sleep(void);
int rsf_fixture_vulkan_dispatch_run(bool fsr4, bool xess);
#ifdef __cplusplus
}
#endif
