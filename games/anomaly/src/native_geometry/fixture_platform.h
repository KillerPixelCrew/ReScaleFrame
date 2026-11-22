// SPDX-License-Identifier: MIT
#pragma once

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

void rsf_test_anomaly_geometry_run_hooks(const rsf_test_anomaly_geometry_hooks*);
void rsf_test_anomaly_geometry_run_contract(const rsf_test_anomaly_geometry_contract*);

#ifdef __cplusplus
}
#endif
