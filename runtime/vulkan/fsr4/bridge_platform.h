// SPDX-License-Identifier: MIT
#pragma once
#include "bridge_api.h"
#include "../../backends/fsr/src/ffx_version.h"

// Private Windows x64 C ABI between the native FSR 4 leaves and the Rust owner
// in owner.rs. Rust owns validation, provider policy, fence sequencing and
// checked retirement and command recording. The native capsule retains the
// SDK and INT8 compatibility objects and the COM references returned by Rust.
// The host serializes every call on one bridge.

inline constexpr uint32_t RSF_FSR4SR_PLATFORM_ABI_VERSION = 1;

struct ID3D12Resource;

// Objects currently held by the native capsule.
enum : uint32_t {
    RSF_FSR4SR_DEVICE = 1u << 0,
    RSF_FSR4SR_QUEUE = 1u << 1,
    RSF_FSR4SR_EFFECT = 1u << 2,
    RSF_FSR4SR_LOCAL_FENCE = 1u << 3,
    RSF_FSR4SR_EVENT = 1u << 4,
    RSF_FSR4SR_SHARED_FENCE = 1u << 5,
};

// Create options; the native leaf maps them to FFX_UPSCALE_ENABLE_* flags.
enum : uint32_t {
    RSF_FSR4SR_DEPTH_INVERTED = 1u << 0,
    RSF_FSR4SR_DEPTH_INFINITE = 1u << 1,
    RSF_FSR4SR_MOTION_JITTERED = 1u << 2,
};

/// @brief Fixed-layout reconstruction frame shared with the managed FSR4 ABI.
/// @details All four resource pointers borrow caller-owned shared images in COMMON state. Vulkan signals readyValue after input copies; DX12 signals doneValue after reconstruction/output handoff.
struct VulkanStoryFsr4Frame {
    ID3D12Resource* color;
    ID3D12Resource* depth;
    ID3D12Resource* motion;
    ID3D12Resource* output;
    uint32_t renderWidth, renderHeight, displayWidth, displayHeight;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fov;
    uint32_t reset;
    uint64_t readyValue, doneValue;
};

struct rsf_fsr4sr_request {
    uint64_t luid;
    const uint16_t* directory; // Null selects the bridge module's directory.
    uint32_t render_width, render_height, output_width, output_height;
    uint32_t flags;
};

struct rsf_fsr4sr_context {
    uint64_t provider;
    uint32_t render_width, render_height, output_width, output_height;
    uint32_t flags;
};

struct rsf_fsr4sr_texture {
    uint64_t width;
    uint32_t height, format; // DXGI_FORMAT
};

// Partially populated on failure. The receiving capsule owns every returned
// COM reference and event, including those created before a later refusal.
struct rsf_fsr4sr_command_objects {
    void* completion;
    void* event;
    void* allocators[3];
    void* list;
};

// One validated FidelityFX dispatch recorded on allocator `slot`.
struct rsf_fsr4sr_dispatch {
    void* color; void* depth; void* motion; void* output;
    uint32_t render_width, render_height, output_width, output_height;
    float jitter_x, jitter_y, motion_scale_x, motion_scale_y;
    float frame_time_ms, pre_exposure, near_plane, far_plane;
    float vertical_fov, sharpness;
    uint32_t reset, slot;
};

struct rsf_fsr4sr_native;

extern "C" {
// Every entry is required. The table is static and outlives every bridge.
struct rsf_fsr4sr_platform {
    uint32_t struct_size, abi_version;
    rsf_fsr4sr_native* (*allocate)();
    void (*release)(rsf_fsr4sr_native*);
    uint32_t (*objects)(const rsf_fsr4sr_native*);
    int32_t (*find_adapter)(rsf_fsr4sr_native*, uint64_t luid, uint32_t* vendor);
    int32_t (*create_device)(rsf_fsr4sr_native*);
    uint32_t (*own_directory)(uint16_t* path, uint32_t capacity);
    int32_t (*load)(rsf_fsr4sr_native*, const uint16_t* path);
    uint32_t (*acquire_compatibility)(rsf_fsr4sr_native*);
    void (*release_compatibility)(rsf_fsr4sr_native*);
    int32_t (*versions)(rsf_fsr4sr_native*, rsf_ffx_version*, uint64_t* count);
    int32_t (*create_context)(rsf_fsr4sr_native*, const rsf_fsr4sr_context*);
    // parsed: 1 when the name has the "%u.%u.%u" shape, including family 0.
    int32_t (*provider)(rsf_fsr4sr_native*, rsf_ffx_version*, uint32_t* parsed);
    int32_t (*create_commands)(rsf_fsr4sr_native*);
    int32_t (*plan)(rsf_fsr4sr_native*, uint32_t quality, uint32_t width, uint32_t height, uint32_t*, uint32_t*);
    int32_t (*create_image)(rsf_fsr4sr_native*, uint32_t, uint32_t, uint32_t, uint32_t, void**, void**);
    int32_t (*create_fence)(rsf_fsr4sr_native*, void**);
    void (*describe)(void* resource, rsf_fsr4sr_texture*);
    uint64_t (*completed)(rsf_fsr4sr_native*);
    int32_t (*wait_event)(rsf_fsr4sr_native*, uint64_t value, uint32_t milliseconds);
    int32_t (*record)(rsf_fsr4sr_native*, const rsf_fsr4sr_dispatch*);
    int32_t (*wait_shared)(rsf_fsr4sr_native*, uint64_t value);
    void (*execute)(rsf_fsr4sr_native*);
    int32_t (*signal)(rsf_fsr4sr_native*, uint64_t value, uint32_t shared);
    int32_t (*destroy_context)(rsf_fsr4sr_native*);
};

uint32_t rsf_core_fsr4sr_candidate_vendor(uint32_t vendor);
int32_t rsf_core_fsr4sr_create(const rsf_fsr4sr_platform*, const rsf_fsr4sr_request*, rsf_fsr4_bridge**);
int32_t rsf_core_fsr4sr_create_public(const rsf_fsr4sr_platform*, const rsf_fsr4_bridge_create*, rsf_fsr4_bridge**);
int32_t rsf_core_fsr4sr_info(const rsf_fsr4_bridge*, rsf_fsr4_bridge_info*);
int32_t rsf_core_fsr4sr_sharpness(rsf_fsr4_bridge*, float);
int32_t rsf_core_fsr4sr_plan(rsf_fsr4_bridge*, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_fsr4sr_create_image(rsf_fsr4_bridge*, uint32_t, uint32_t, uint32_t, uint32_t, void**, void**);
int32_t rsf_core_fsr4sr_create_fence(rsf_fsr4_bridge*, void**);
int32_t rsf_core_fsr4sr_evaluate_legacy(rsf_fsr4_bridge*, const VulkanStoryFsr4Frame*);
int32_t rsf_core_fsr4sr_evaluate(rsf_fsr4_bridge*, const rsf_fsr4_bridge_frame*);
int32_t rsf_core_fsr4sr_wait_idle(rsf_fsr4_bridge*);
int32_t rsf_core_fsr4sr_prepare_destroy(rsf_fsr4_bridge*);
int32_t rsf_core_fsr4sr_destroy(rsf_fsr4_bridge*);
}

static_assert(sizeof(VulkanStoryFsr4Frame) == 96 && offsetof(VulkanStoryFsr4Frame, readyValue) == 80);
static_assert(sizeof(rsf_fsr4sr_request) == 40 && offsetof(rsf_fsr4sr_request, flags) == 32);
static_assert(sizeof(rsf_fsr4sr_context) == 32 && offsetof(rsf_fsr4sr_context, flags) == 24);
static_assert(sizeof(rsf_fsr4sr_texture) == 16 && offsetof(rsf_fsr4sr_texture, format) == 12);
static_assert(sizeof(rsf_fsr4sr_command_objects) == 6 * sizeof(void*));
static_assert(sizeof(rsf_fsr4sr_dispatch) == 96 && offsetof(rsf_fsr4sr_dispatch, render_width) == 32 &&
    offsetof(rsf_fsr4sr_dispatch, frame_time_ms) == 64 && offsetof(rsf_fsr4sr_dispatch, reset) == 88);
static_assert(sizeof(rsf_fsr4sr_platform) == 8 + 24 * sizeof(void*) &&
    offsetof(rsf_fsr4sr_platform, allocate) == 8 && offsetof(rsf_fsr4sr_platform, create_context) == 88 &&
    offsetof(rsf_fsr4sr_platform, destroy_context) == 192);
