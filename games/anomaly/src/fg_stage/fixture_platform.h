// SPDX-License-Identifier: MIT
#pragma once
#include "../fg_stage_platform.h"
#include <rescaleframe/native_cpu.h>

typedef struct rsf_anomaly_fg_fixture_platform {
    uint32_t struct_size;
    rsf_anomaly_fg_state_scope state_scope;
    void (*release_cpu)(void);
    void (*ping_message)(uint32_t);
    int (*read_cpu)(uint64_t, uint64_t, rsf_native_cpu_frame*);
    int (*cpu_counters)(rsf_native_cpu_counters*);
} rsf_anomaly_fg_fixture_platform;
