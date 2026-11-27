// SPDX-License-Identifier: MIT
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rsf_test_anomaly_geometry_scenario {
    const void* context;
    void (*run)(const void*);
} rsf_test_anomaly_geometry_scenario;

typedef struct rsf_test_anomaly_geometry_hooks {
    rsf_test_anomaly_geometry_scenario lifecycle, flush_capture, phase, identities, foliage;
    rsf_test_anomaly_geometry_scenario vertex_constants, contexts, coverage, retirement;
    rsf_test_anomaly_geometry_scenario release, guards;
} rsf_test_anomaly_geometry_hooks;

typedef struct rsf_test_anomaly_geometry_contract {
    rsf_test_anomaly_geometry_scenario capture, history, coverage, inheritance, sink;
} rsf_test_anomaly_geometry_contract;

// The selected build's Rust-owned facts: executable identity, image size and the engine globals the fixtures seed.
typedef struct rsf_test_anomaly_geometry_profile {
    const char* label;
    const char* executable;
    uint32_t image_size, phase, viewport, frame, world, vertex_constants, hierarchy_vtable;
} rsf_test_anomaly_geometry_profile;

void rsf_test_anomaly_geometry_run_hooks(const rsf_test_anomaly_geometry_hooks*);
void rsf_test_anomaly_geometry_run_contract(const rsf_test_anomaly_geometry_contract*);
bool rsf_test_anomaly_geometry_profile_get(size_t build, rsf_test_anomaly_geometry_profile*);

#ifdef __cplusplus
}
#endif
