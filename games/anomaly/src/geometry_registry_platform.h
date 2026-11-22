// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

enum {
    RSF_ANOMALY_VISUAL_MISSING,
    RSF_ANOMALY_VISUAL_UNIQUE,
    RSF_ANOMALY_VISUAL_AMBIGUOUS,
};
enum {
    RSF_ANOMALY_PASS_MAIN, RSF_ANOMALY_PASS_SHADOW, RSF_ANOMALY_PASS_UNKNOWN, RSF_ANOMALY_PASS_COUNT,
};
enum {
    RSF_ANOMALY_TYPE_STATIC, RSF_ANOMALY_TYPE_PROGRESSIVE, RSF_ANOMALY_TYPE_SKELETON_ST,
    RSF_ANOMALY_TYPE_SKELETON_PM, RSF_ANOMALY_TYPE_TREE_ST, RSF_ANOMALY_TYPE_TREE_PM, RSF_ANOMALY_TYPE_COUNT,
};
enum {
    RSF_ANOMALY_DRAW_IDENTIFIED, RSF_ANOMALY_DRAW_IGNORED_SHADOW, RSF_ANOMALY_DRAW_IGNORED_STATIC_WORLD,
    RSF_ANOMALY_DRAW_UNIDENTIFIED_DYNAMIC, RSF_ANOMALY_DRAW_UNIDENTIFIED_DEFORMED,
    RSF_ANOMALY_DRAW_UNIDENTIFIED_AMBIGUOUS, RSF_ANOMALY_DRAW_UNIDENTIFIED_UNREADABLE,
    RSF_ANOMALY_DRAW_UNIDENTIFIED_NO_CAPTURE, RSF_ANOMALY_DRAW_IDENTIFIED_FOLIAGE, RSF_ANOMALY_DRAW_COUNT,
};

typedef struct rsf_anomaly_geometry_draw {
    int32_t pass, visual_type, lookup;
    uint32_t capturing, world_readable, world_identity, foliage_identity;
} rsf_anomaly_geometry_draw;

typedef struct rsf_anomaly_geometry_key {
    uint64_t owner, visual, generation;
} rsf_anomaly_geometry_key;

typedef struct rsf_anomaly_geometry_probe {
    uint32_t lookup;
    rsf_anomaly_geometry_key key;
} rsf_anomaly_geometry_probe;

typedef struct rsf_anomaly_geometry_registry rsf_anomaly_geometry_registry;
typedef struct rsf_anomaly_geometry_retirements rsf_anomaly_geometry_retirements;
typedef void (*rsf_anomaly_geometry_retired)(void*, const rsf_anomaly_geometry_key*);

#ifdef __cplusplus
extern "C" {
#endif
rsf_anomaly_geometry_registry* rsf_anomaly_geometry_registry_create(void);
void rsf_anomaly_geometry_registry_destroy(rsf_anomaly_geometry_registry*);
uint64_t rsf_anomaly_geometry_register(rsf_anomaly_geometry_registry*, uint64_t owner, uint64_t visual);
uint64_t rsf_anomaly_geometry_find(const rsf_anomaly_geometry_registry*, uint64_t owner, uint64_t visual);
void rsf_anomaly_geometry_probe_visual(const rsf_anomaly_geometry_registry*, uint64_t visual,
    rsf_anomaly_geometry_probe*);
uint64_t rsf_anomaly_geometry_retire(rsf_anomaly_geometry_registry*, uint64_t owner, uint64_t visual);
size_t rsf_anomaly_geometry_retire_all(rsf_anomaly_geometry_registry*, rsf_anomaly_geometry_retired, void*);

rsf_anomaly_geometry_retirements* rsf_anomaly_geometry_retirements_create(void);
void rsf_anomaly_geometry_retirements_destroy(rsf_anomaly_geometry_retirements*);
void rsf_anomaly_geometry_enqueue(rsf_anomaly_geometry_retirements*, const rsf_anomaly_geometry_key*);
size_t rsf_anomaly_geometry_drain(rsf_anomaly_geometry_retirements*, rsf_anomaly_geometry_retired, void*);
uint64_t rsf_anomaly_geometry_dropped(const rsf_anomaly_geometry_retirements*);
#ifdef __cplusplus
}
#endif
