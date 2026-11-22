// SPDX-License-Identifier: MIT
#pragma once
#include "engine_shader_includes.h"
#include "motion_device_platform.h"
#include "motion_frame_target.h"
#include "motion_target_platform.h"
#include "native_geometry_platform.h"
#include "shader_frame_layout.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>


namespace rsf::anomaly::motion {
using FrameTarget = rsf_anomaly_motion_frame_target;

// Private static-library ABI. Engine objects borrow their native lifetime.
// Native include operations guard engine calls. Rust opens and owns the pack.
struct PositionViews { void* rtv; void* surface; };
struct TargetFacts { uint32_t width, height, samples, layers; };
struct DrawFacts {
    const rsf_anomaly_geometry_snapshot* capture;
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, kind, verdict, view, domain;
    bool in_visual, detail;
};
struct ShaderObjects { void* vs; void* hs; void* ds; void* ps; void* gs; };
struct MotionDrawState { float viewport[4], jitter[4]; };
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
    const rsf_anomaly_engine_include_ops* includes;
    void (*shader_profile)(const void*, rsf_anomaly_engine_shader_profile*);
    void* (*frame_layout)(rsf_anomaly_shader_frame_layout*);
    rsf_anomaly_shader_read read_engine;
};
inline constexpr uint32_t motion_platform_abi = 12;

extern "C" {
void rsf_anomaly_motion_stage_configure(const MotionPlatform*, const wchar_t*, rsf_dump_log_fn, void*) noexcept;
bool rsf_anomaly_motion_stage_install(const MotionPlatform*, void*, uint32_t, const void*, uint32_t, const char*) noexcept;
bool rsf_anomaly_motion_stage_quiesce() noexcept;
bool rsf_anomaly_motion_stage_enabled() noexcept;
void rsf_anomaly_motion_stage_report(rsf_dump_log_fn, void*) noexcept;
void rsf_anomaly_motion_stage_frame_target(FrameTarget*) noexcept;
using MotionGuardedCopy = bool (*)(void*, const void*, size_t);
bool rsf_anomaly_motion_resolve_draws(MotionGuardedCopy, void*, void* (*)[4]) noexcept;
bool rsf_anomaly_motion_position_views(MotionGuardedCopy, void*, uint32_t, uint32_t, uint32_t, PositionViews*) noexcept;
void rsf_anomaly_motion_viewport(void*, float (*)[4]) noexcept;
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
