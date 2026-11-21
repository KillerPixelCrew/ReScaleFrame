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
typedef struct rsf_fsr12_config {
    void* chain;
    void* user;
    void* hudless;
    uint64_t sequence;
    uint32_t width, height, enabled, copy_present;
} rsf_fsr12_config;

typedef struct rsf_fsr12_capture_resource {
    void* resource;
    uint32_t state;
} rsf_fsr12_capture_resource;

typedef struct rsf_fsr12_generated {
    void* command;
    uint64_t sequence;
    uint32_t count, reset;
    rsf_fsr12_capture_resource input, outputs[4];
} rsf_fsr12_generated;

typedef struct rsf_fsr12_ops {
    const rsf_gen12_checks* check;
    const struct rsf_core_capture12_platform* capture;
    rsf_backend_result (*resolve)(const rsf_generation_setup*, void* module, void* latency, void**);
    void (*close)(void*);
    rsf_backend_result (*versions)(void*, const rsf_generation_setup*, struct rsf_ffx_version*, uint64_t*);
    uint32_t (*surface_format)(uint32_t);
    rsf_backend_result (*create_generation)(void*, const rsf_gen12_extent*, const rsf_fg_status*, const char*, void**);
    rsf_backend_result (*provider_matches)(void*, void*, uint64_t);
    rsf_backend_result (*create_chain)(void*, const rsf_generation_setup*, void**, void**);
    uint32_t (*destroy_context)(void*, void**);
    uint32_t (*wait)(void*, void*);
    int32_t (*chain_extent)(void*, rsf_gen12_extent*);
    uint32_t (*whole_texture)(const rsf_backend_resource*);
    uint32_t (*configure)(void*, void*, const rsf_fsr12_config*);
    uint32_t (*prepare)(void*, void*, void*, const rsf_fg_frame*, uint64_t, uint32_t, float, float);
    uint32_t (*generate)(void*, void*, void*, rsf_fsr12_generated*);
    uint32_t (*copy_present)(void*);
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
static_assert(sizeof(rsf_fsr12_config) == 48);
static_assert(sizeof(rsf_fsr12_generated) == 104);
static_assert(sizeof(rsf_fsr12_ops) == 18 * sizeof(void*));
static_assert(sizeof(rsf_xeg12_version) == 12 && sizeof(rsf_xeg12_present) == 12);
static_assert(sizeof(rsf_xeg12_ops) == 24 * sizeof(void*));
#endif
