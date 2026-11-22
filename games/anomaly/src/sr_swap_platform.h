// SPDX-License-Identifier: MIT
#pragma once
// Private ABI between the native SR world-window leaf (sr_stage.cpp) and the Rust swap owner (sr_swap.rs).
// tools/generate-rust-bindings.py generates sr_swap_abi.rs from this header; sr_swap.h names the world slots of the
// same structures. Engine addresses are opaque here: every engine read and write goes through the platform's guarded
// copies, and every D3D or engine call through its callbacks.
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define RSF_ANOMALY_SWAP_SLOT_COUNT 9
#define RSF_ANOMALY_SWAP_STAGE1_SLOT_COUNT 2
#define RSF_ANOMALY_SWAP_DESC_SIZE 44   /* D3D11_TEXTURE2D_DESC, cached in CTexture::desc */
#define RSF_ANOMALY_SWAP_ABSENT_COUNT 4
#define RSF_ANOMALY_SWAP_TEXTURE_STAGE_COUNT 6

#define RSF_ANOMALY_SWAP_SKIP_OFF 0
#define RSF_ANOMALY_SWAP_SKIP_SCOPE 1
#define RSF_ANOMALY_SWAP_SKIP_MSAA 2
#define RSF_ANOMALY_SWAP_SKIP_LAYOUT 3
#define RSF_ANOMALY_SWAP_SKIP_TWINS 4
#define RSF_ANOMALY_SWAP_SKIP_DEPTH 5
#define RSF_ANOMALY_SWAP_SKIP_COUNT 6

#define RSF_ANOMALY_SWAP_RESTORE_SEAM 0
#define RSF_ANOMALY_SWAP_RESTORE_COMBINE_END 1
#define RSF_ANOMALY_SWAP_RESTORE_STALE_PREPARE 2
#define RSF_ANOMALY_SWAP_RESTORE_SHUTDOWN 3
#define RSF_ANOMALY_SWAP_RESTORE_ROLLBACK 4
#define RSF_ANOMALY_SWAP_RESTORE_FRAME_END 5
#define RSF_ANOMALY_SWAP_RESTORE_COUNT 6

#define RSF_ANOMALY_SWAP_MAKE_CREATED 0
#define RSF_ANOMALY_SWAP_MAKE_FAILED 1
#define RSF_ANOMALY_SWAP_MAKE_DEPTH 2
#define RSF_ANOMALY_SWAP_MAKE_REFUSED 3

// The swapped values of one render target: its CRT record and the CTexture it is bound through.
typedef struct rsf_anomaly_swap_fields {
    void* surface;
    void* rtv;
    void* dsv;
    void* uav;
    uint32_t width, height, format;   // CRT dwWidth, dwHeight, fmt (D3DFORMAT)
    void* texture_surface;            // CTexture::pSurface
    void* desc_cache;                 // CTexture::desc_cache
    uint8_t desc[RSF_ANOMALY_SWAP_DESC_SIZE];
    void* srv;                        // CTexture::m_pSRView
} rsf_anomaly_swap_fields;

// One engine target: where its CRT and CTexture live, and the values read from them. `present` is false for an
// optional target whose ref_rt is null.
typedef struct rsf_anomaly_swap_target {
    void* crt;
    void* texture;
    rsf_anomaly_swap_fields fields;
    bool present;
} rsf_anomaly_swap_target;

// What the H1 hook read for this frame. A zero `stage` means Stage 1.
typedef struct rsf_anomaly_swap_facts {
    void* render_target;              // CRenderTarget* passed to phase_scene_prepare
    void* backend;                    // &RCache, for the texture-slot cache invalidation; null skips it
    void* device;                     // HW.pDevice identity
    uint64_t generation;              // reset generation (native observer)
    uint32_t device_width, device_height;   // Device.dwWidth/dwHeight at H1: the output size
    bool scope, msaa;
    // Stage 2. Stage 1 ignores everything below.
    uint32_t stage;
    uint32_t render_width, render_height;
    void* device_size;                // &Device.dwWidth (dwHeight follows at +4)
    void* base_zb;                    // &HW.pBaseZB
    void* base_rt;                    // &HW.pBaseRT
    void* ssao_viewport;              // &set_viewport's static D3D11_VIEWPORT; null leaves it alone
    void* ssao_viewport_guard;
    // Stage 3, applied only when the Stage 2 window opens.
    bool jitter;                      // jitter Device.mProject by (jitter_x, jitter_y) render pixels (+X right, +Y down)
    float jitter_x, jitter_y;
    void* device_view;                // &Device.mView, &Device.mProject, &Device.mFullTransform (Fmatrix, 64 bytes each)
    void* device_project;
    void* device_full_transform;
    bool mip_bias;                    // add mip_bias_delta to the sampler cache's bias for the window
    float mip_bias_delta;
    void* sampler_bias;               // &SSManager.m_uiMipLODBias: read only, changed through set_mip_bias
} rsf_anomaly_swap_facts;

// What the host must create on a twin-key change.
typedef struct rsf_anomaly_swap_spec {
    bool present;
    uint32_t width, height;
    bool promote;                     // R16G16B16A16_FLOAT instead of the engine format
} rsf_anomaly_swap_spec;

typedef struct rsf_anomaly_swap_request {
    uint32_t stage;
    rsf_anomaly_swap_target engine[RSF_ANOMALY_SWAP_SLOT_COUNT];
    rsf_anomaly_swap_spec spec[RSF_ANOMALY_SWAP_SLOT_COUNT];
    void* engine_dsv;                 // Stage 2: HW.pBaseZB, validated by the host
    void* engine_rtv;                 // Stage 2: HW.pBaseRT (the back buffer view)
    uint32_t render_width, render_height, output_width, output_height;
} rsf_anomaly_swap_request;

typedef struct rsf_anomaly_swap_twins {
    rsf_anomaly_swap_fields targets[RSF_ANOMALY_SWAP_SLOT_COUNT];
    void* depth_dsv;                  // Stage 2 depth twin: D24_UNORM_S8_UINT view of an R24G8_TYPELESS texture
    void* depth_srv;                  // R24_UNORM_X8_TYPELESS view of the same texture
    uint32_t depth_width, depth_height;
    void* base_rt;                    // Stage 2: render-size scratch RTV standing in for HW.pBaseRT inside the window
} rsf_anomaly_swap_twins;

// The seam pass of Stage 2: render-size inputs, output-size engine targets.
typedef struct rsf_anomaly_swap_upscale {
    const rsf_anomaly_swap_fields* generic0_twin;   // FP16, engine low tonemap space
    const rsf_anomaly_swap_fields* position_twin;
    const rsf_anomaly_swap_fields* generic1_twin;
    void* depth_srv;
    const rsf_anomaly_swap_fields* generic0;        // engine UNORM8 originals at output size
    const rsf_anomaly_swap_fields* position;
    const rsf_anomaly_swap_fields* generic1;
    void* engine_dsv;
    uint32_t render_width, render_height, output_width, output_height;
    bool spatial_recovery; // World camera recovery requires the spatial seam pass.
} rsf_anomaly_swap_upscale;

typedef struct rsf_anomaly_swap_counters {
    uint64_t prepares, eligible, swapped, seams, seams_without_window, round_trips, round_trip_failures;
    uint64_t combine_ends, rm_near_in_window, twin_generations, twin_drops, foreign_changes, write_faults;
    uint64_t allocation_failures, viewports, ssao_viewport_swaps;
    // Stage 3. jittered: windows whose main projection carries the jitter. fp_*: rmNear calls inside a jittered window
    // (jittered a freshly built first-person projection / found one the owner already wrote / left an unknown one).
    uint64_t jittered, jitter_invalid, fp_jittered, fp_already, fp_foreign, projection_restores;
    uint64_t mip_applied, mip_restored, mip_unchanged;
    uint64_t sun_calls, sun_unjittered, sun_restored, sun_refused;
    uint64_t position_corrected, position_stock, position_refused;
    uint64_t skipped[RSF_ANOMALY_SWAP_SKIP_COUNT];
    uint64_t restores[RSF_ANOMALY_SWAP_RESTORE_COUNT];
} rsf_anomaly_swap_counters;

// The verified engine layout shared by both executable profiles (sr_profile.rs), supplied once at creation.
typedef struct rsf_anomaly_swap_world_target {
    uint32_t offset;                  // ref_rt (CRT*) inside CRenderTarget
    uint32_t d3d_format;              // 0 accepts R16F or R32F
    bool optional, half_allowed;
} rsf_anomaly_swap_world_target;

typedef struct rsf_anomaly_swap_range {
    uint32_t offset, count;
} rsf_anomaly_swap_range;

typedef struct rsf_anomaly_swap_layout {
    uint32_t struct_size;
    uint32_t crt_surface, crt_rtv, crt_dsv, crt_uav, crt_texture, crt_width, crt_height, crt_format;
    uint32_t texture_surface, texture_desc_cache, texture_desc, texture_srv;
    uint32_t target_width, target_height;   // CRenderTarget::dwWidth/dwHeight
    uint32_t backend_texture_list, backend_ctable;
    rsf_anomaly_swap_range backend_textures[RSF_ANOMALY_SWAP_TEXTURE_STAGE_COUNT];   // PS, VS, GS, HS, DS, CS
    uint32_t ssao_viewport_width;     // the height follows at +4
    uint32_t format_a16b16g16r16f, format_r16f, format_r32f;
    rsf_anomaly_swap_world_target world[RSF_ANOMALY_SWAP_SLOT_COUNT];   // in slot order
    uint32_t absent[RSF_ANOMALY_SWAP_ABSENT_COUNT];   // CRenderTarget slots that must be null in a Stage 2 window
} rsf_anomaly_swap_layout;

// Native leaf callbacks. read/write are guarded copies of engine memory. The rest are the D3D11 and engine calls the
// owner sequences: twin creation and release, the seam passes, the viewport, R_xforms::set_P, SetMipLODBias and the
// deferred position constant correction.
typedef struct rsf_anomaly_swap_platform {
    void* user;
    bool (*read)(void* user, const void* address, void* output, size_t size);
    bool (*write)(void* user, void* address, const void* input, size_t size);
    uint32_t (*make_twins)(void* user, const rsf_anomaly_swap_request* request, rsf_anomaly_swap_twins* twins);
    void (*drop_twins)(void* user);
    bool (*round_trip)(void* user, const rsf_anomaly_swap_fields* twin, const rsf_anomaly_swap_fields* engine);
    bool (*upscale)(void* user, const rsf_anomaly_swap_upscale* inputs);
    void (*set_viewport)(void* user, uint32_t width, uint32_t height);
    bool (*upload_projection)(void* user, const void* matrix);
    bool (*set_mip_bias)(void* user, float value);
    // 0: clean refusal, 1: corrected, 2: incomplete rollback.
    uint32_t (*correct_position_constant)(void* user, void* constant, float x, float y);
} rsf_anomaly_swap_platform;

// What the native leaf reads between calls. Owned by the swap handle and updated by every call.
typedef struct rsf_anomaly_swap_view {
    rsf_anomaly_swap_counters counters;
    rsf_anomaly_swap_twins twins;
    rsf_anomaly_swap_target engine[RSF_ANOMALY_SWAP_SLOT_COUNT];   // saved at the last exchange
    const char* refusal;              // null until the first refusal; static text
    uint32_t stage;                   // stage and sizes of the last window that opened
    uint32_t render_width, render_height, output_width, output_height;
    bool open, jitter_applied, mip_bias_applied;
    float window_mip_bias, saved_mip_bias;
} rsf_anomaly_swap_view;

typedef struct rsf_anomaly_swap rsf_anomaly_swap;

#ifdef __cplusplus
extern "C" {
#endif
// Null for a short layout or a platform with a missing callback. The platform and its user outlive the handle.
rsf_anomaly_swap* rsf_anomaly_swap_create(const rsf_anomaly_swap_layout*, const rsf_anomaly_swap_platform*);
// The stage supplies the seam operation in Rust, so round_trip and upscale may be null.
rsf_anomaly_swap* rsf_anomaly_swap_create_scoped(const rsf_anomaly_swap_layout*, const rsf_anomaly_swap_platform*);
// Shutdown must succeed before normal unload. Destroy alone does not retire retained twins.
void rsf_anomaly_swap_destroy(rsf_anomaly_swap*);
const rsf_anomaly_swap_view* rsf_anomaly_swap_view_of(const rsf_anomaly_swap*);
void rsf_anomaly_swap_set_enabled(rsf_anomaly_swap*, bool enabled);
void rsf_anomaly_swap_close_stale(rsf_anomaly_swap*);
bool rsf_anomaly_swap_prepare(rsf_anomaly_swap*, const rsf_anomaly_swap_facts*);
void rsf_anomaly_swap_prepared(rsf_anomaly_swap*);
bool rsf_anomaly_swap_seam(rsf_anomaly_swap*);
void rsf_anomaly_swap_combine_end(rsf_anomaly_swap*);
void rsf_anomaly_swap_frame_end(rsf_anomaly_swap*);
void rsf_anomaly_swap_rm_near(rsf_anomaly_swap*);
bool rsf_anomaly_swap_sun_begin(rsf_anomaly_swap*);
void rsf_anomaly_swap_sun_end(rsf_anomaly_swap*, bool suspended);
void rsf_anomaly_swap_position_constant(rsf_anomaly_swap*, void* constant);
bool rsf_anomaly_swap_shutdown(rsf_anomaly_swap*);
const char* rsf_anomaly_swap_skip_name(uint32_t);
const char* rsf_anomaly_swap_restore_name(uint32_t);
#ifdef __cplusplus
}
#endif
