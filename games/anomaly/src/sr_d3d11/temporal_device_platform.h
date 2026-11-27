/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/motion_decode.h>
#include <rescaleframe/motion_resolve.h>

typedef struct rsf_anomaly_temporal_fixture_graphics {
    rsf_motion_decode_result (*decode_create)(void*, const rsf_motion_decode_setup*, rsf_motion_decode**);
    rsf_motion_decode_result (*decode_run)(rsf_motion_decode*, void*, void*, const rsf_motion_decode_params*);
    void* (*decode_texture)(rsf_motion_decode*);
    void (*decode_destroy)(rsf_motion_decode*);
    int (*resolve_create)(void*, uint32_t, uint32_t, rsf_motion_resolve**);
    int (*resolve_run)(rsf_motion_resolve*, void*, void*, void*, const rsf_motion_resolve_params*);
    void* (*resolve_motion)(rsf_motion_resolve*);
    void (*resolve_destroy)(rsf_motion_resolve*);
} rsf_anomaly_temporal_fixture_graphics;
