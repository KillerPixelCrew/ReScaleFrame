/* SPDX-License-Identifier: MIT */
#pragma once
#include <rescaleframe/frame_generation.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Private Rust/native bridge. SDK descriptors and COM calls stay in the native leaves. */
typedef struct rsf_gen12_checks {
    rsf_backend_result (*setup)(const rsf_generation_setup*, void**, void**);
    rsf_backend_result (*frame)(const rsf_fg_frame*, void*);
    rsf_backend_result (*command)(void*, void*);
    uint32_t (*streamline_loaded)(void);
    void* (*load_library)(const rsf_generation_setup*, const uint16_t*);
} rsf_gen12_checks;

typedef struct rsf_gen12_extent {
    uint32_t width, height, format;
} rsf_gen12_extent;

struct rsf_core_capture12_platform;
struct rsf_ffx_version;
/* API versions the native leaf was compiled against, for Rust's startup records. */
typedef struct rsf_fsr12_sdk {
    uint32_t generation[3], swapchain[3];
} rsf_fsr12_sdk;

/* Generation context plan. Rust selects the version, extents and FFX create flags. */
typedef struct rsf_fsr12_generation {
    uint64_t version_id;
    uint32_t width, height, max_render_width, max_render_height, format, flags;
} rsf_fsr12_generation;

/* Prepare constants planned in Rust; native copies them into the typed SDK descriptor. */
typedef struct rsf_fsr12_prepare {
    void* command;
    void* depth;
    void* motion;
    uint64_t sequence;
    uint32_t render_width, render_height;
    float jitter[2], motion_scale[2];
    float frame_time_ms, near_plane, far_plane, vertical_fov, view_space_to_meters;
    uint32_t reset;
    float position[3], right[3], up[3], forward[3];
} rsf_fsr12_prepare;

/* SDK exports Rust resolved and admitted, in rsf_fsr12_entry_index order. */
typedef void (*rsf_fsr12_entry)(void);
enum rsf_fsr12_entry_index {
    RSF_FSR12_CREATE_CONTEXT,
    RSF_FSR12_DESTROY_CONTEXT,
    RSF_FSR12_QUERY,
    RSF_FSR12_DISPATCH,
    RSF_FSR12_CONFIGURE,
    RSF_FSR12_ENTRY_COUNT
};

/* Configure plan. Rust decides the generation rectangle and which SDK callbacks
   are installed; both callbacks receive `user`. */
typedef struct rsf_fsr12_config {
    void* chain;
    void* user;
    void* hudless;
    uint64_t sequence;
    int32_t left, top, width, height;
    uint32_t enabled, generation_callback, present_callback;
} rsf_fsr12_config;

/* An SDK resource with its raw FFX_API_RESOURCE_STATE_* bits; Rust maps the state. */
typedef struct rsf_fsr12_capture_resource {
    void* resource;
    uint32_t state;
} rsf_fsr12_capture_resource;

/* Raw dispatch facts: all four SDK output slots, bounded by `count` in Rust. */
typedef struct rsf_fsr12_generated {
    void* command;
    uint64_t sequence;
    uint32_t count, reset;
    rsf_fsr12_capture_resource input, outputs[4];
} rsf_fsr12_generated;

/* Raw present-callback facts; Rust admits the copy. */
typedef struct rsf_fsr12_present {
    void* command;
    rsf_fsr12_capture_resource source, output;
} rsf_fsr12_present;

typedef struct rsf_fsr12_ops {
    const rsf_gen12_checks* check;
    const struct rsf_core_capture12_platform* capture;
    const rsf_fsr12_sdk* sdk;
    rsf_backend_result (*resolve)(const rsf_generation_setup*, void* module, void* latency, void**);
    void (*close)(void*);
    void (*bind)(void*, const rsf_fsr12_entry*);
    rsf_backend_result (*versions)(void*, const rsf_generation_setup*, struct rsf_ffx_version*, uint64_t*);
    uint32_t (*surface_format)(uint32_t);
    /* The next three return raw FFX codes; Rust owns admission and records. */
    uint32_t (*create_generation)(void*, const rsf_fsr12_generation*, void**);
    uint32_t (*provider_version)(void*, void*, uint64_t*);
    /* Fails only before the SDK call (DXGI factory); FFX result and the SDK's chain come back. */
    rsf_backend_result (*create_chain)(void*, const rsf_generation_setup*, void**, void**, uint32_t*);
    uint32_t (*destroy_context)(void*, void**);
    uint32_t (*wait)(void*, void*);
    int32_t (*chain_extent)(void*, rsf_gen12_extent*);
    uint32_t (*whole_texture)(const rsf_backend_resource*);
    uint32_t (*configure)(void*, void*, const rsf_fsr12_config*);
    uint32_t (*prepare)(void*, void*, const rsf_fsr12_prepare*);
    uint32_t (*generate)(void*, void*, void*, rsf_fsr12_generated*);
    void (*present_facts)(void*, rsf_fsr12_present*);
    /* command, output, output D3D12 state, source, source D3D12 state. */
    void (*copy)(void*, void*, uint32_t, void*, uint32_t);
    int32_t (*present_count)(void*, uint32_t*);
} rsf_fsr12_ops;

typedef struct rsf_xeg12_version {
    uint32_t major, minor, patch;
} rsf_xeg12_version;

typedef struct rsf_xeg12_present {
    int32_t interpolation;
    uint32_t enabled, frames;
} rsf_xeg12_present;

typedef struct rsf_xeg12_ops {
    const rsf_gen12_checks* check;
    rsf_backend_result (*resolve)(const rsf_generation_setup*, void* module, void* latency, void**);
    void (*close)(void*);
    int32_t (*create_fg)(void*, void*, void**);
    int32_t (*create_xell)(void*, void*, void**);
    int32_t (*destroy_fg)(void*, void*);
    int32_t (*destroy_xell)(void*, void*);
    int32_t (*max_count)(void*, void*, uint32_t*);
    int32_t (*bind_xell)(void*, void*, void*);
    void (*version)(void*, rsf_xeg12_version*);
    rsf_backend_result (*create_chain)(void*, void*, const rsf_generation_setup*, uint32_t, void**);
    int32_t (*compose)(void*, void*, uint32_t);
    int32_t (*count)(void*, void*, uint32_t);
    int32_t (*enabled)(void*, void*, uint32_t);
    int32_t (*sleep_mode)(void*, void*, uint32_t, uint32_t);
    int32_t (*sleep)(void*, void*, uint32_t);
    int32_t (*marker)(void*, void*, uint32_t, rsf_latency_marker);
    int32_t (*present_id)(void*, void*, uint32_t);
    int32_t (*tag)(void*, void*, void*, uint32_t, uint32_t, const rsf_backend_resource*);
    int32_t (*constants)(void*, void*, uint32_t, const rsf_fg_frame*, uint32_t);
    int32_t (*present_status)(void*, void*, rsf_xeg12_present*);
    void* (*create_fence)(void*);
    void* (*create_event)(void);
    int32_t (*drain)(void*, void*, void*, uint64_t);
} rsf_xeg12_ops;

const rsf_generation_provider* rsf_core_fsr12_provider(void);
/* The query receives the ffxQuery export Rust resolved, as its first argument. */
uint32_t rsf_core_fsr12_probe(const rsf_gen12_checks*, const rsf_generation_setup*,
    rsf_backend_result (*query)(void*, const rsf_generation_setup*, struct rsf_ffx_version*, uint64_t*));
rsf_backend_result rsf_core_fsr12_create(const rsf_fsr12_ops*, const rsf_generation_setup*, void**, void**);
uint32_t rsf_core_fsr12_generate(void*, void*);
uint32_t rsf_core_fsr12_present(void*, void*, uint32_t);
const rsf_generation_provider* rsf_core_xeg12_provider(void);
rsf_backend_result rsf_core_xeg12_create(const rsf_xeg12_ops*, const rsf_generation_setup*, void**, void**);

#ifdef __cplusplus
}
static_assert(sizeof(rsf_gen12_checks) == 5 * sizeof(void*));
static_assert(sizeof(rsf_gen12_extent) == 12);
static_assert(sizeof(rsf_fsr12_sdk) == 24 && sizeof(rsf_fsr12_generation) == 32);
static_assert(sizeof(rsf_fsr12_prepare) == 128);
static_assert(sizeof(rsf_fsr12_config) == 64 && RSF_FSR12_ENTRY_COUNT == 5);
static_assert(sizeof(rsf_fsr12_generated) == 104 && sizeof(rsf_fsr12_present) == 40);
static_assert(sizeof(rsf_fsr12_ops) == 21 * sizeof(void*));
static_assert(sizeof(rsf_xeg12_version) == 12 && sizeof(rsf_xeg12_present) == 12);
static_assert(sizeof(rsf_xeg12_ops) == 24 * sizeof(void*));
#endif
