// SPDX-License-Identifier: MIT
#pragma once
#include "geometry_registry_platform.h"
#include "motion_device_platform.h"
#include "motion_target_platform.h"
#include "shader_loader_platform.h"
#include <rescaleframe/game_frame.h>
#include <rescaleframe/texture_dump.h>

namespace rsf::anomaly::motion {
struct FrameTarget;
struct FoliageLayout;

// Private static-library ABI. Engine objects borrow their native lifetime.
// Pack, request and reflection handles use the corresponding destroy leaf.
struct WindowFacts {
    void* device;
    void* context;
    void* backend;
    void* position_rtv;
    void* position_surface;
    uint64_t generation;
    uint32_t frame;
    uint8_t view;
};
struct TargetFacts { uint32_t width, height, samples, layers; };
struct DrawFacts {
    void* capture;
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, kind, verdict, view, domain;
    bool in_visual, detail;
};
struct ShaderObjects { void* vs; void* hs; void* ds; void* ps; void* gs; };
struct SnapshotFacts {
    rsf_anomaly_geometry_key key;
    const void* world;
    const void* bones;
    size_t bone_count;
    uint64_t palette_layout;
    uint8_t domain, kind;
};
struct MotionDrawState { float viewport[4], jitter[4]; };
struct ShaderEntry { const void* entry; rsf_anomaly_shader_bytes name; uint8_t stage, family; };
struct ShaderResult {
    void* reflection;
    rsf_anomaly_shader_bytes detail;
    int32_t skin;
    uint8_t outcome;
};
struct PackInfo { rsf_anomaly_shader_bytes name_space; size_t roots; };
struct PatchCounters { uint64_t intercepted, built, cache_hits, refused, refusals[11]; };
using Redraw = void (*)(void*, void*);
using Retire = void (*)(void*, const rsf_anomaly_geometry_key*);
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
    bool (*window_facts)(void*, WindowFacts*);
    int32_t (*target_facts)(void*, TargetFacts*);
    void* (*backend)();
    void* (*context)();
    void* (*shader_device)();
    void (*clear_target)(void*, void*);
    void (*bound_shaders)(void*, ShaderObjects*, bool);
    void (*set_shaders)(void*, const ShaderObjects*, bool);
    uint8_t (*replay)(const rsf_anomaly_motion_device_view*, void*, Redraw, void*);
    bool (*camera)(float (*)[16], float (*)[16]);
    bool (*world)(float (*)[16]);
    bool (*snapshot)(void*, SnapshotFacts*);
    size_t (*drain)(Retire, void*);
    bool (*read_constant)(uint32_t, uint32_t, uint32_t, void*, uint32_t);
    bool (*write_constant)(uint32_t, uint32_t, uint32_t, const void*, uint32_t);
    void (*draw_state)(void*, MotionDrawState*);
    void* (*open_pack)(void*, const void*, const wchar_t*, const wchar_t*, PackInfo*, char*, size_t);
    void (*close_pack)(void*);
    void (*activate_pack)(void*);
    void (*attach_device)(void*, void*);
    int32_t (*dump_shaders)(void*, const wchar_t*);
    void (*patch_counters)(void*, PatchCounters*);
    void* (*decode_request)(const char*, const void*, uint32_t, const void*, const void*, uint8_t*, rsf_anomaly_shader_bytes*);
    void (*drop_request)(void*);
    bool (*copy_bytecode)(void*);
    bool (*find_entry)(void*, const uint8_t (*)[64], ShaderEntry*);
    void (*prepare_shader)(void*, void*, const void*, ShaderResult*);
    void (*drop_reflection)(void*);
    bool (*foliage_layout)(void*, uint8_t, FoliageLayout*);
    bool (*link_shaders)(const void*, const void*, const void*, const void*);
    void* (*create_shader)(void*, void*, void*, void*, bool, bool*);
    bool (*sr_capture)();
};
inline constexpr uint32_t motion_platform_abi = 1;

extern "C" {
void rsf_anomaly_motion_stage_configure(const MotionPlatform*, const wchar_t*, rsf_dump_log_fn, void*) noexcept;
bool rsf_anomaly_motion_stage_install(const MotionPlatform*, void*, uint32_t, const void*, uint32_t, const char*) noexcept;
bool rsf_anomaly_motion_stage_quiesce() noexcept;
bool rsf_anomaly_motion_stage_admits() noexcept;
bool rsf_anomaly_motion_stage_enabled() noexcept;
void rsf_anomaly_motion_stage_report(rsf_dump_log_fn, void*) noexcept;
void rsf_anomaly_motion_stage_frame_target(FrameTarget*) noexcept;
void rsf_anomaly_motion_stage_cancel_draw() noexcept;
void rsf_anomaly_motion_stage_draw(void*, Redraw, void*) noexcept;
void rsf_anomaly_motion_stage_open(void*) noexcept;
void rsf_anomaly_motion_stage_close() noexcept;
void rsf_anomaly_motion_stage_flush(const DrawFacts*) noexcept;
void rsf_anomaly_motion_stage_attach() noexcept;
void rsf_anomaly_motion_stage_shader(uint8_t, const char*, const void*, uint32_t, rsf_anomaly_shader_bytes,
    void*, const void*, const void*) noexcept;
}
}
