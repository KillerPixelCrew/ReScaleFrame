// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
#include "ffx_api/ffx_api.h"
#include "ffx_api/vk/ffx_api_vk.h"

// Private Windows x64 C ABI between the native FidelityFX SDK 1.1.4 Vulkan
// leaves in bridge.cpp and the Rust SR owner in owner.rs. Rust owns the
// runtime binding, SR admission and refusal codes, frame-format validation,
// the frame-time default and checked retirement. The native capsule keeps the
// typed FidelityFX descriptors, the Vulkan loader lookup with its promoted-core
// fallback and the SDK resource mapping. Frame generation and WSI stay native
// and read the same Rust-retained SDK table.

inline constexpr uint32_t RSF_FSR3SR_PLATFORM_ABI_VERSION = 1;

/// @brief Borrowed level-zero Vulkan image metadata in the FidelityFX bridge C ABI.
/// @details The caller owns the VkImage and supplies its extent/format; this record owns no view, allocation or synchronization.
struct VulkanStoryFsr3Image {
    VkImage image;
    uint32_t width, height;
    VkFormat format;
};
/// @brief Fixed-layout SR dispatch record shared with the managed FSR3 ABI.
/// @details The 184-byte record contains six 24-byte image records; jitter begins at byte 152. Color/output use RGBA16F, depth D32F, motion RG16F and independent material reactive/composition masks R8_UNORM. The host keeps all resources live through GPU completion.
struct VulkanStoryFsr3Frame {
    VkCommandBuffer commands;
    /// @brief Borrowed SR scene color, depth, motion, output and separate render-resolution material/composition resources.
    VulkanStoryFsr3Image color, depth, motion, output, reactive, composition;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fovRadians;
    uint32_t reset;
};
static_assert(sizeof(VulkanStoryFsr3Image) == 24);
static_assert(sizeof(VulkanStoryFsr3Frame) == 184);
static_assert(offsetof(VulkanStoryFsr3Frame, jitterX) == 152);
static_assert(offsetof(VulkanStoryFsr3Frame, composition) == 128);

/// @brief Rust-owned SR context. The export ABI passes it as an opaque pointer.
struct VulkanStoryFsr3Context;
/// @brief Native capsule holding one FidelityFX reconstruction effect and its creation descriptors.
struct rsf_fsr3sr_native;

/// @brief Process-retained SDK function table bound once by VulkanStoryFsr3Open.
/// @details All entries are null until a successful bind and never change afterwards. Frame generation and WSI read it through rsf_core_fsr3_sdk.
struct rsf_fsr3_sdk {
    void* module;
    PfnFfxCreateContext create;
    PfnFfxDestroyContext destroy;
    PfnFfxQuery query;
    PfnFfxDispatch dispatch;
    PfnFfxConfigure configure;
};

/// @brief Validated SR creation request; device handles are borrowed.
struct rsf_fsr3sr_create {
    VkDevice device;
    VkPhysicalDevice physical;
    uint32_t render_width, render_height, width, height;
};

// bind_vulkan result bits. Rust requires both.
enum : uint32_t {
    RSF_FSR3SR_VULKAN_LOADER = 1u << 0,
    RSF_FSR3SR_VULKAN_BUFFER_REQUIREMENTS = 1u << 1,
};

extern "C" {
// Every entry is required. The table is static and outlives every context.
struct rsf_fsr3sr_platform {
    uint32_t struct_size, abi_version;
    // GetProcAddress on the borrowed SDK module.
    void* (*resolve)(void* module, const char* name);
    rsf_fsr3sr_native* (*allocate)();
    void (*release)(rsf_fsr3sr_native*);
    // Rebinds the shared vulkan-1.dll vkGetDeviceProcAddr and probes the
    // buffer-requirements entry through the promoted-core fallback.
    uint32_t (*bind_vulkan)(VkDevice device);
    // Fills the HDR/auto-exposure upscale and Vulkan backend descriptors and creates the effect.
    int32_t (*create)(rsf_fsr3sr_native*, PfnFfxCreateContext, const rsf_fsr3sr_create*);
    int32_t (*plan)(PfnFfxQuery, uint32_t display_width, uint32_t display_height, uint32_t quality,
        uint32_t* render_width, uint32_t* render_height);
    // Maps the six images and records the upscale dispatch with the supplied frame time.
    int32_t (*evaluate)(rsf_fsr3sr_native*, PfnFfxDispatch, const VulkanStoryFsr3Frame*, float frame_time_ms);
    uint32_t (*has_effect)(const rsf_fsr3sr_native*);
    int32_t (*destroy)(rsf_fsr3sr_native*, PfnFfxDestroyContext);
};

const rsf_fsr3_sdk* rsf_core_fsr3_sdk();
int32_t rsf_core_fsr3_open(const rsf_fsr3sr_platform*, void* runtime);
int32_t rsf_core_fsr3_plan(const rsf_fsr3sr_platform*, uint32_t, uint32_t, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_fsr3_create(const rsf_fsr3sr_platform*, const rsf_fsr3sr_create*, VulkanStoryFsr3Context**);
int32_t rsf_core_fsr3_evaluate(VulkanStoryFsr3Context*, const VulkanStoryFsr3Frame*);
int32_t rsf_core_fsr3_destroy(VulkanStoryFsr3Context*);
}

static_assert(sizeof(rsf_fsr3_sdk) == 48 && offsetof(rsf_fsr3_sdk, dispatch) == 32);
static_assert(sizeof(rsf_fsr3sr_create) == 32 && offsetof(rsf_fsr3sr_create, render_width) == 16);
static_assert(sizeof(rsf_fsr3sr_platform) == 8 + 9 * sizeof(void*) &&
    offsetof(rsf_fsr3sr_platform, resolve) == 8 && offsetof(rsf_fsr3sr_platform, create) == 40 &&
    offsetof(rsf_fsr3sr_platform, destroy) == 72);
