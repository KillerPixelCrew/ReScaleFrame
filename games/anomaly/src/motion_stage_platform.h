// SPDX-License-Identifier: MIT
#pragma once
#include "motion_device_platform.h"
#include "motion_frame_target.h"
#include "motion_target_platform.h"
#include "shader_loader_platform.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>

struct rsf_shader_patch_api;

namespace rsf::anomaly::geometry { struct SnapshotFacts; }

namespace rsf::anomaly::motion {
using FrameTarget = rsf_anomaly_motion_frame_target;
struct FoliageLayout;

// Private static-library ABI. Engine objects borrow their native lifetime.
// Pack and reflection handles use the corresponding destroy leaf.
struct PositionViews { void* rtv; void* surface; };
struct TargetFacts { uint32_t width, height, samples, layers; };
struct DrawFacts {
    const geometry::SnapshotFacts* capture;
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, kind, verdict, view, domain;
    bool in_visual, detail;
};
struct ShaderObjects { void* vs; void* hs; void* ds; void* ps; void* gs; };
struct MotionDrawState { float viewport[4], jitter[4]; };
struct ShaderResult {
    void* reflection;
    rsf_anomaly_shader_bytes detail;
    int32_t skin;
    uint8_t outcome;
};
struct ShaderCreation {
    const char* target;
    const void* bytecode;
    const char* file;
    void* const* result;
    const void* return_address;
    const void* stack;
    int32_t status;
    uint32_t size;
};
// These pointers are owned by the pack until close_pack.
struct PackInfo {
    const rsf_anomaly_shader_admitted* manifest;
    const rsf_shader_patch_api* api;
};
struct PatchCounters { uint64_t intercepted, built, cache_hits, refused, refusals[11]; };
enum : uint8_t { NoMotionDraw, RestoreMotionDraw, ReplayMotionDraw };
// Begin initializes the opaque owner for the Restore and Replay routes.
struct MotionDrawScope { uint64_t storage[22]; };
struct MotionPlatform {
    uint32_t struct_size, abi_version;
    const rsf_anomaly_motion_device_platform* device;
    const rsf_anomaly_motion_bind_platform* bind;
    rsf_anomaly_motion_bind_layout bind_layout;
    const char* (*guards)(void*, uint32_t, const void*);
    bool (*geometry_active)();
    void (*observe_flush)(bool);
    void (*foliage_identity)(bool);
    bool (*install_hooks)(void*, uint32_t, const void*);
    bool (*disable_hooks)();
    bool (*hooks_created)();
    bool (*install_draw_hooks)(void*);
    void* (*render_target)();
    bool (*position_views)(void*, PositionViews*);
    bool (*native_frame)(uint32_t*);
    bool (*render_view)(uint8_t*);
    uint64_t (*reset_generation)();
    bool (*options_word3)(uint32_t*);
    bool (*target_facts)(void*, TargetFacts*);
    void* (*backend)();
    void* (*context)();
    void* (*shader_device)();
    void (*clear_target)(void*, void*);
    void (*bound_shaders)(void*, ShaderObjects*, bool);
    void (*set_shaders)(void*, const ShaderObjects*, bool);
    const rsf_anomaly_motion_replay_platform* replay;
    bool (*camera)(float (*)[16], float (*)[16]);
    bool (*world)(float (*)[16]);
    bool (*read_constant)(uint32_t, uint32_t, uint32_t, void*, uint32_t);
    bool (*write_constant)(uint32_t, uint32_t, uint32_t, const void*, uint32_t);
    void (*draw_state)(void*, MotionDrawState*);
    void* (*open_pack)(void*, const void*, const wchar_t*, const wchar_t*, PackInfo*, char*, size_t);
    void (*close_pack)(void*);
    void (*attach_device)(void*, void*);
    int32_t (*dump_shaders)(void*, const wchar_t*);
    void (*patch_counters)(void*, PatchCounters*);
    void* (*frame_layout)(rsf_anomaly_shader_frame_layout*);
    rsf_anomaly_shader_read read_engine;
    void (*prepare_shader)(void*, const rsf_anomaly_shader_request*, const rsf_anomaly_shader_entry*, ShaderResult*);
    void (*drop_reflection)(void*);
    bool (*foliage_layout)(void*, uint8_t, FoliageLayout*);
    bool (*link_shaders)(const void*, const void*, const void*, const void*);
    void* (*create_shader)(void*, const char*, void*, void*, bool, bool*);
};
inline constexpr uint32_t motion_platform_abi = 10;

extern "C" {
void rsf_anomaly_motion_stage_configure(const MotionPlatform*, const wchar_t*, rsf_dump_log_fn, void*) noexcept;
bool rsf_anomaly_motion_stage_install(const MotionPlatform*, void*, uint32_t, const void*, uint32_t, const char*) noexcept;
bool rsf_anomaly_motion_stage_quiesce() noexcept;
bool rsf_anomaly_motion_stage_enabled() noexcept;
void rsf_anomaly_motion_stage_report(rsf_dump_log_fn, void*) noexcept;
void rsf_anomaly_motion_stage_frame_target(FrameTarget*) noexcept;
bool rsf_anomaly_motion_target_facts(void*, TargetFacts*) noexcept;
void rsf_anomaly_motion_bound_shaders(void*, ShaderObjects*, bool) noexcept;
void rsf_anomaly_motion_set_shaders(void*, const ShaderObjects*, bool) noexcept;
void rsf_anomaly_motion_stage_cancel_draw() noexcept;
uint8_t rsf_anomaly_motion_stage_draw_begin(void*, MotionDrawScope*) noexcept;
void rsf_anomaly_motion_stage_draw_end(MotionDrawScope*, bool completed) noexcept;
bool rsf_anomaly_motion_stage_replay_begin(const MotionDrawScope*, rsf_anomaly_motion_replay_scope*) noexcept;
void rsf_anomaly_motion_stage_replay_end(const MotionDrawScope*, rsf_anomaly_motion_replay_scope*, bool completed) noexcept;
void rsf_anomaly_motion_stage_open(void*) noexcept;
void rsf_anomaly_motion_stage_close() noexcept;
void rsf_anomaly_motion_stage_flush(const DrawFacts*) noexcept;
void rsf_anomaly_motion_stage_attach() noexcept;
void rsf_anomaly_motion_stage_shader(const ShaderCreation*) noexcept;
}
}
