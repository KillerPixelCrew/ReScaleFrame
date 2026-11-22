// SPDX-License-Identifier: MIT
#pragma once
// Private static-library ABI between the motion stage (motion/stage.rs, motion/stage/abi.rs) and its C11 leaf
// (motion_stage_platform.c). Engine objects borrow their native lifetime. Native include operations guard engine
// calls. Rust opens and owns the pack and selects the per-build motion, SR and native records
// (motion_profile.rs, sr_profile.rs, native_profile.rs); the leaf receives the addresses it reads.
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

typedef struct rsf_anomaly_motion_position_pair {
    void* rtv;
    void* surface;
} rsf_anomaly_motion_position_pair;

typedef struct rsf_anomaly_motion_target_info {
    uint32_t width, height, samples, layers;
} rsf_anomaly_motion_target_info;

// `capture` borrows the geometry producer's snapshot of this draw for the call, or is null.
typedef struct rsf_anomaly_motion_draw_facts {
    const rsf_anomaly_geometry_snapshot* capture;
    uint64_t visual;
    int64_t tree_lod;
    uint8_t pass, kind, verdict, view, domain;
    bool in_visual, detail;
} rsf_anomaly_motion_draw_facts;

typedef struct rsf_anomaly_motion_shader_objects {
    void* vs;
    void* hs;
    void* ds;
    void* ps;
    void* gs;
} rsf_anomaly_motion_shader_objects;

typedef struct rsf_anomaly_motion_draw_state {
    float viewport[4], jitter[4];
} rsf_anomaly_motion_draw_state;

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

// Motion detour order of `install_hooks` sites (motion_profile.rs SITE_*).
enum {
    RSF_ANOMALY_MOTION_SITE_CREATE_SHADER,
    RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_BEGIN,
    RSF_ANOMALY_MOTION_SITE_PHASE_SCENE_END,
    RSF_ANOMALY_MOTION_SITE_COUNT,
};

typedef bool (*rsf_anomaly_motion_guarded_copy)(void*, const void*, size_t);

// The installed build's engine addresses, image-relative (motion/stage/native.rs). The leaf keeps a copy.
typedef struct rsf_anomaly_motion_engine {
    size_t build;                // native_profile.rs build index, for the geometry view read
    uint32_t hw_device_rva;      // HW.pDevice
    uint32_t hw_context_rva;     // HW.pContext
    uint32_t backend_rva;        // RCache (CBackend)
    uint32_t render_target_rva;  // RImplementation.Target (CRenderTarget*)
    uint32_t options_word3_rva;  // fourth u32 of CRender::o
    uint32_t device_frame_rva;   // Device.dwFrame
    uint32_t position_offset;    // CRenderTarget::rt_Position
    uint32_t crt_rtv_offset;     // CRT::pRT
    uint32_t crt_surface_offset; // CRT::pSurface
    uint32_t view_rva;           // RCache.xforms.m_v
    uint32_t projection_rva;     // RCache.xforms.m_p
} rsf_anomaly_motion_engine;

#define RSF_ANOMALY_MOTION_PLATFORM_ABI 13u
typedef struct rsf_anomaly_motion_platform {
    uint32_t struct_size, abi_version;
    const rsf_anomaly_motion_device_platform* device;
    const rsf_anomaly_motion_bind_platform* bind;
    // Guarded engine-memory copy behind the guard image.
    rsf_anomaly_motion_guarded_copy copy;
    bool (*geometry_active)(void);
    void (*observe_flush)(bool);
    void (*foliage_identity)(bool);
    // Adopts `engine` for the leaf reads, then installs the lent sites with the leaf's detours and trampoline slots
    // filled in RSF_ANOMALY_MOTION_SITE_* order.
    bool (*install_hooks)(void* image, uint32_t size, const rsf_anomaly_motion_engine* engine,
        const rsf_anomaly_hook_site*, size_t count);
    bool (*disable_hooks)(void);
    bool (*hooks_created)(void);
    bool (*install_draw_hooks)(void*);
    void* (*render_target)(void);
    bool (*position_views)(void*, rsf_anomaly_motion_position_pair*);
    bool (*native_frame)(uint32_t*);
    bool (*render_view)(uint8_t*);
    uint64_t (*reset_generation)(void);
    bool (*options_word3)(uint32_t*);
    bool (*target_facts)(void*, rsf_anomaly_motion_target_info*);
    void* (*backend)(void);
    void* (*context)(void);
    void* (*shader_device)(void);
    void (*clear_target)(void*, void*);
    void (*bound_shaders)(void*, rsf_anomaly_motion_shader_objects*, bool);
    void (*set_shaders)(void*, const rsf_anomaly_motion_shader_objects*, bool);
    const rsf_anomaly_motion_replay_platform* replay;
    bool (*camera)(float (*)[16], float (*)[16]);
    bool (*world)(float (*)[16]);
    bool (*read_constant)(uint32_t, uint32_t, uint32_t, void*, uint32_t);
    bool (*write_constant)(uint32_t, uint32_t, uint32_t, const void*, uint32_t);
    void (*draw_state)(void*, rsf_anomaly_motion_draw_state*);
    const rsf_anomaly_engine_include_ops* includes;
    rsf_anomaly_shader_read read_engine;
} rsf_anomaly_motion_platform;

void rsf_anomaly_motion_stage_configure(const rsf_anomaly_motion_platform*, const wchar_t*, rsf_dump_log_fn,
    void*) RSF_MOTION_NOEXCEPT;
// `build` is the native_profile.rs index the observer admitted.
bool rsf_anomaly_motion_stage_install(const rsf_anomaly_motion_platform*, void* image, size_t build) RSF_MOTION_NOEXCEPT;
bool rsf_anomaly_motion_resolve_draws(rsf_anomaly_motion_guarded_copy, void*, void* (*)[4]) RSF_MOTION_NOEXCEPT;
bool rsf_anomaly_motion_position_views(rsf_anomaly_motion_guarded_copy, void*, uint32_t, uint32_t, uint32_t,
    rsf_anomaly_motion_position_pair*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_viewport(void*, float (*)[4]) RSF_MOTION_NOEXCEPT;
bool rsf_anomaly_motion_target_facts(void*, rsf_anomaly_motion_target_info*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_bound_shaders(void*, rsf_anomaly_motion_shader_objects*, bool) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_set_shaders(void*, const rsf_anomaly_motion_shader_objects*, bool) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_cancel_draw(void) RSF_MOTION_NOEXCEPT;
uint8_t rsf_anomaly_motion_stage_draw_begin(void*, rsf_anomaly_motion_draw_scope*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_draw_end(rsf_anomaly_motion_draw_scope*, bool completed) RSF_MOTION_NOEXCEPT;
bool rsf_anomaly_motion_stage_replay_begin(const rsf_anomaly_motion_draw_scope*,
    rsf_anomaly_motion_replay_scope*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_replay_end(const rsf_anomaly_motion_draw_scope*, rsf_anomaly_motion_replay_scope*,
    bool completed) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_open(void*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_close(void) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_flush(const rsf_anomaly_motion_draw_facts*) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_attach(void) RSF_MOTION_NOEXCEPT;
void rsf_anomaly_motion_stage_shader(const rsf_anomaly_motion_shader_creation*) RSF_MOTION_NOEXCEPT;

#ifdef __cplusplus
}
#endif
#undef RSF_MOTION_NOEXCEPT
