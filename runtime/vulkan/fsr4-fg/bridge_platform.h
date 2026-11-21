// SPDX-License-Identifier: MIT
#pragma once
#include "bridge_api.h"
#include "../../backends/fsr/src/ffx_version.h"

enum : uint32_t {
    RSF_FSR4FG_DEVICE = 1u << 0,
    RSF_FSR4FG_GENERATION = 1u << 1,
    RSF_FSR4FG_CHAIN = 1u << 2,
    RSF_FSR4FG_SWAPCHAIN = 1u << 3,
    RSF_FSR4FG_LOCAL_FENCE = 1u << 4,
    RSF_FSR4FG_SHARED_FENCE = 1u << 5,
};

struct rsf_fsr4fg_texture {
    uint64_t width;
    uint32_t height, format, dimension, layers, samples;
};

struct rsf_fsr4fg_native;

extern "C" {
typedef void (*rsf_fsr4fg_generated_fn)(void*, int32_t result, uint32_t count);

// Native owners and retained SDK descriptors stay behind the void* handle.
// The operations table and callback are valid until release.
struct rsf_fsr4fg_platform {
    rsf_fsr4fg_native* (*allocate)(void* telemetry, rsf_fsr4fg_generated_fn);
    void (*release)(rsf_fsr4fg_native*);
    int32_t (*open_device)(rsf_fsr4fg_native*, uint64_t luid, uint32_t* vendor);
    int32_t (*load)(rsf_fsr4fg_native*, const uint16_t* path);
    int32_t (*versions)(rsf_fsr4fg_native*, rsf_ffx_version*, uint64_t* count);
    int32_t (*create_generation)(rsf_fsr4fg_native*, const rsf_fsr4fg_create*, uint64_t provider);
    int32_t (*get_provider)(rsf_fsr4fg_native*, uint64_t*);
    int32_t (*create_commands)(rsf_fsr4fg_native*);
    uint32_t (*objects)(rsf_fsr4fg_native*);
    int32_t (*start)(rsf_fsr4fg_native*, void* window, uint32_t width, uint32_t height);
    int32_t (*create_image)(rsf_fsr4fg_native*, uint32_t width, uint32_t height, uint32_t format, void** handle, void** resource);
    int32_t (*create_fence)(rsf_fsr4fg_native*, void** handle);
    void (*describe_texture)(void* resource, rsf_fsr4fg_texture*);
    int32_t (*reset_commands)(rsf_fsr4fg_native*);
    int32_t (*close_commands)(rsf_fsr4fg_native*);
    int32_t (*configure)(rsf_fsr4fg_native*, uint64_t sequence, uint32_t width, uint32_t height, uint32_t enabled, void* hudless);
    void (*shader_read)(rsf_fsr4fg_native*, void* resource, uint32_t enter);
    int32_t (*prepare)(rsf_fsr4fg_native*, const rsf_fsr4fg_frame*, uint64_t sequence, uint32_t reset);
    int32_t (*copy_to_backbuffer)(rsf_fsr4fg_native*, void* color);
    int32_t (*wait_shared)(rsf_fsr4fg_native*, uint64_t);
    void (*execute)(rsf_fsr4fg_native*);
    int32_t (*present)(rsf_fsr4fg_native*, uint32_t vsync);
    int32_t (*wait_presents)(rsf_fsr4fg_native*);
    int32_t (*signal)(rsf_fsr4fg_native*, uint64_t, uint32_t shared);
    int32_t (*wait_local)(rsf_fsr4fg_native*, uint64_t);
    int32_t (*get_present_count)(rsf_fsr4fg_native*, uint32_t*);
    int32_t (*destroy_generation)(rsf_fsr4fg_native*);
    void (*release_chain)(rsf_fsr4fg_native*);
    int32_t (*destroy_swapchain)(rsf_fsr4fg_native*);
};

uint64_t rsf_core_fsr4fg_select_version(const rsf_ffx_version*, uint64_t count);
int32_t rsf_core_fsr4fg_probe(const rsf_fsr4fg_platform*, uint64_t, const uint16_t*, rsf_fsr4fg_info*);
int32_t rsf_core_fsr4fg_create(const rsf_fsr4fg_platform*, const rsf_fsr4fg_create*, rsf_fsr4fg_bridge**);
int32_t rsf_core_fsr4fg_info(const rsf_fsr4fg_bridge*, rsf_fsr4fg_info*);
int32_t rsf_core_fsr4fg_start(rsf_fsr4fg_bridge*, void*);
int32_t rsf_core_fsr4fg_create_image(rsf_fsr4fg_bridge*, uint32_t, uint32_t, uint32_t, void**, void**);
int32_t rsf_core_fsr4fg_create_fence(rsf_fsr4fg_bridge*, void**);
int32_t rsf_core_fsr4fg_present(rsf_fsr4fg_bridge*, const rsf_fsr4fg_frame*, rsf_fsr4fg_present_status*);
int32_t rsf_core_fsr4fg_drain(rsf_fsr4fg_bridge*);
int32_t rsf_core_fsr4fg_prepare_destroy(rsf_fsr4fg_bridge*);
int32_t rsf_core_fsr4fg_destroy(rsf_fsr4fg_bridge*);
}

static_assert(sizeof(rsf_fsr4fg_platform) == 29 * sizeof(void*));
static_assert(sizeof(rsf_fsr4fg_texture) == 32 && offsetof(rsf_fsr4fg_texture, dimension) == 16);
