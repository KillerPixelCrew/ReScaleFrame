// SPDX-License-Identifier: MIT
#pragma once
// Private static-library ABI between the motion stage (motion/stage.rs, motion/stage/abi.rs) and its C11 leaf
// (motion_stage_platform.c). Engine objects borrow their native lifetime. Native include operations guard engine
// calls. Rust opens and owns the pack, selects the per-build motion, SR and native records
// (motion_profile.rs, sr_profile.rs, native_profile.rs), reads the engine and installs the leaf's detours.
#include "engine_shader_includes.h"
#include "motion_device_platform.h"
#include "motion_stage.h"
#include "motion_target_platform.h"
#include "native_geometry_platform.h"
#include "native_hooks.h"
#include "shader_frame_layout.h"
#include <rescaleframe/texture_dump.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <wchar.h>

#ifdef __cplusplus
#define RSF_MOTION_NOEXCEPT noexcept
extern "C" {
#else
#define RSF_MOTION_NOEXCEPT
#endif

typedef struct rsf_anomaly_motion_shader_objects {
    void* vs;
    void* hs;
    void* ds;
    void* ps;
    void* gs;
} rsf_anomaly_motion_shader_objects;

typedef struct rsf_anomaly_motion_shader_creation {
    const char* target;
    const void* bytecode;
    const char* file;
    void* const* result;
    const void* return_address;
    const void* stack;
    int32_t status;
    uint32_t size;
} rsf_anomaly_motion_shader_creation;

enum { RSF_ANOMALY_MOTION_NO_DRAW, RSF_ANOMALY_MOTION_RESTORE_DRAW, RSF_ANOMALY_MOTION_REPLAY_DRAW };
// Begin initializes the opaque owner for the Restore and Replay routes.
typedef struct rsf_anomaly_motion_draw_scope {
    uint64_t storage[22];
} rsf_anomaly_motion_draw_scope;

// Motion detour order (motion_profile.rs SITE_*).
enum {
    RSF_ANOMALY_MOTION_SITE_CREATE_SHADER,
    RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_BEGIN,
    RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_END,
    RSF_ANOMALY_MOTION_SITE_COUNT,
};

// The SEH-guarded engine includer (shader_loader/runtime_pack.rs).
bool rsf_anomaly_motion_native_include_open(void* table, const char*, const void**, uint32_t*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_native_include_close(void* table, const void*) RSF_MOTION_NOEXCEPT;

// Rust entries of the detours (motion/stage.rs).
void rsf_anomaly_motion_stage_cancel_draw(void) RSF_MOTION_NOEXCEPT;
uint8_t rsf_anomaly_motion_stage_draw_begin(void*, rsf_anomaly_motion_draw_scope*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_draw_end(rsf_anomaly_motion_draw_scope*, bool completed) RSF_MOTION_NOEXCEPT;
bool rsf_anomaly_motion_stage_replay_begin(const rsf_anomaly_motion_draw_scope*,
    rsf_anomaly_motion_replay_scope*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_replay_end(const rsf_anomaly_motion_draw_scope*, rsf_anomaly_motion_replay_scope*,
    bool completed) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_open(void*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_close(void) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_attach(void) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_shader(const rsf_anomaly_motion_shader_creation*) RSF_MOTION_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_MOTION_NOEXCEPT
