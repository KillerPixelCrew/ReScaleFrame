// SPDX-License-Identifier: MIT
#pragma once
#include <cstddef>
#include <cstdint>
#include "ffx_api/ffx_api.h"
#include "ffx_api/vk/ffx_api_vk.h"

// Private Windows x64 C ABI between the native FidelityFX SDK 1.1.4 Vulkan
// leaves in bridge.cpp and the Rust owners in owner.rs. Rust owns the runtime
// binding, every admission and refusal code, frame-format validation, the
// frame-time default, the interpolation/swapchain pairing and checked
// retirement. The native capsules keep the typed FidelityFX effects and
// descriptors, the swapchain replacement function table, the Vulkan loader
// lookup with its promoted-core fallback, the SDK resource mapping and the
// presentation callback shell.

inline constexpr uint32_t RSF_FSR3SR_PLATFORM_ABI_VERSION = 1;
inline constexpr uint32_t RSF_FSR3FG_PLATFORM_ABI_VERSION = 1;

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

/// @brief Fixed-layout interpolation record shared with the managed frame-generation ABI.
/// @details The 248-byte record supplies matching camera/timing resources. A null swapchainContext selects direct interpolation; a nonnull context selects the complete SDK swapchain owner.
struct VulkanStoryFsr3FgFrame {
    VkCommandBuffer commands;
    VulkanStoryFsr3Image color, depth, motion, output, hudless;
    float jitterX, jitterY, deltaMs, nearPlane, farPlane, fovRadians;
    uint64_t frameId;
    uint32_t reset;
    float cameraPos[3], cameraUp[3], cameraRight[3], cameraForward[3];
    VulkanStoryFsr3Image ui;
    /// @brief Borrowed SDK swapchain owner for proxy interpolation, or null for direct interpolation on the host command buffer.
    void* swapchainContext;
};
static_assert(sizeof(VulkanStoryFsr3FgFrame) == 248);
static_assert(offsetof(VulkanStoryFsr3FgFrame, deltaMs) == 136 && offsetof(VulkanStoryFsr3FgFrame, frameId) == 152);
static_assert(offsetof(VulkanStoryFsr3FgFrame, ui) == 216 && offsetof(VulkanStoryFsr3FgFrame, swapchainContext) == 240);
// Rust reads oldSwapchain for the creation refusal.
static_assert(sizeof(VkSwapchainCreateInfoKHR) == 104 && offsetof(VkSwapchainCreateInfoKHR, oldSwapchain) == 96);

/// @brief Rust-owned SR context. The export ABI passes it as an opaque pointer.
struct VulkanStoryFsr3Context;
/// @brief Rust-owned interpolation owner and its borrowed association with an active SDK presentation owner.
/// @details Proxy mode installs a callback referencing its native capsule. Disable and drain presentation before retiring tagged inputs or destroying the effect.
struct VulkanStoryFsr3FgContext;
/// @brief Rust-owned FidelityFX swapchain owner. Acquire, image enumeration, presentation and chain destruction use this same owner.
/// @details activeFg borrows the linked interpolation owner until checked disable/drain succeeds.
struct VulkanStoryFsr3Swapchain;
/// @brief Native capsule holding one FidelityFX reconstruction effect and its creation descriptors.
struct rsf_fsr3sr_native;
/// @brief Native capsule holding one FidelityFX interpolation effect, its creation descriptors and its display extent.
struct rsf_fsr3fg_native;
/// @brief Native capsule holding one FidelityFX swapchain context and its complete replacement Vulkan function table.
struct rsf_fsr3wsi_native;

/// @brief Process-retained SDK function table bound once by VulkanStoryFsr3Open.
/// @details All entries are null until a successful bind and never change afterwards. Rust reads it for admission; the native presentation callback reads it through rsf_core_fsr3_sdk.
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

/// @brief Swapchain creation request; every handle and the creation information are borrowed.
struct rsf_fsr3wsi_create {
    VkPhysicalDevice physical;
    VkDevice device;
    VkQueue game, async, present, acquire;
    const VkSwapchainCreateInfoKHR* info;
    uint32_t queue_family;
};

/// @brief Validated interpolation creation request; device handles are borrowed.
struct rsf_fsr3fg_create {
    VkDevice device;
    VkPhysicalDevice physical;
    uint32_t width, height;
    VkFormat back_buffer_format;
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

// Every entry is required. The table is static and outlives every owner. Rust
// passes the bound SDK entry each leaf calls; only the presentation callback
// reads the table itself.
struct rsf_fsr3fg_platform {
    uint32_t struct_size, abi_version;
    rsf_fsr3wsi_native* (*wsi_allocate)();
    void (*wsi_release)(rsf_fsr3wsi_native*);
    // Fills the four-queue proxy descriptor and creates the context; the SDK writes the chain.
    int32_t (*wsi_create)(rsf_fsr3wsi_native*, PfnFfxCreateContext, const rsf_fsr3wsi_create*, VkSwapchainKHR* chain);
    // Queries the replacement function table.
    int32_t (*wsi_query)(rsf_fsr3wsi_native*, PfnFfxQuery);
    // Whether the five required replacement entries are present.
    uint32_t (*wsi_complete)(const rsf_fsr3wsi_native*);
    uint32_t (*wsi_has_context)(const rsf_fsr3wsi_native*);
    int32_t (*wsi_destroy)(rsf_fsr3wsi_native*, PfnFfxDestroyContext);
    // Waits for the proxy's queued presents.
    int32_t (*wsi_wait)(rsf_fsr3wsi_native*, PfnFfxDispatch);
    // Registers the frame's premultiplied UI with internal double buffering; a null frame clears the registration.
    int32_t (*wsi_register_ui)(rsf_fsr3wsi_native*, PfnFfxConfigure, const VulkanStoryFsr3FgFrame* frame);
    VkResult (*wsi_recreate)(rsf_fsr3wsi_native*, VkDevice, const VkSwapchainCreateInfoKHR*, VkSwapchainKHR*);
    VkResult (*wsi_get_images)(rsf_fsr3wsi_native*, VkDevice, VkSwapchainKHR, uint32_t*, VkImage*);
    VkResult (*wsi_acquire)(rsf_fsr3wsi_native*, VkDevice, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
    VkResult (*wsi_present)(rsf_fsr3wsi_native*, VkQueue, const VkPresentInfoKHR*);
    // -1 when the replacement destroy entry is absent, otherwise 0 after the call.
    int32_t (*wsi_destroy_chain)(rsf_fsr3wsi_native*, VkDevice, VkSwapchainKHR);
    // Zero when the SDK exposes no present-count entry.
    uint64_t (*wsi_last_present_count)(rsf_fsr3wsi_native*, VkSwapchainKHR);
    rsf_fsr3fg_native* (*fg_allocate)();
    void (*fg_release)(rsf_fsr3fg_native*);
    // Rebinds the shared vulkan-1.dll vkGetDeviceProcAddr; nonzero when it resolved.
    uint32_t (*fg_bind_vulkan)();
    // Fills the interpolation, HUD-less RGBA and Vulkan backend descriptors and creates the effect.
    int32_t (*fg_create)(rsf_fsr3fg_native*, PfnFfxCreateContext, const rsf_fsr3fg_create*);
    uint32_t (*fg_has_effect)(const rsf_fsr3fg_native*);
    int32_t (*fg_destroy)(rsf_fsr3fg_native*, PfnFfxDestroyContext);
    // Configures interpolation off for the chain.
    int32_t (*fg_disable)(rsf_fsr3fg_native*, PfnFfxConfigure, VkSwapchainKHR chain);
    // Configures interpolation on: a null chain selects direct mode, otherwise the proxy callback.
    int32_t (*fg_configure)(rsf_fsr3fg_native*, PfnFfxConfigure, const VulkanStoryFsr3FgFrame*, VkSwapchainKHR chain);
    // Records depth/motion/camera preparation with the supplied frame time.
    int32_t (*fg_prepare)(rsf_fsr3fg_native*, PfnFfxDispatch, const VulkanStoryFsr3FgFrame*, float frame_time_ms);
    // Records one direct generated output.
    int32_t (*fg_generate)(rsf_fsr3fg_native*, PfnFfxDispatch, const VulkanStoryFsr3FgFrame*);
};

const rsf_fsr3_sdk* rsf_core_fsr3_sdk();
int32_t rsf_core_fsr3_open(const rsf_fsr3sr_platform*, void* runtime);
int32_t rsf_core_fsr3_plan(const rsf_fsr3sr_platform*, uint32_t, uint32_t, uint32_t, uint32_t*, uint32_t*);
int32_t rsf_core_fsr3_create(const rsf_fsr3sr_platform*, const rsf_fsr3sr_create*, VulkanStoryFsr3Context**);
int32_t rsf_core_fsr3_evaluate(VulkanStoryFsr3Context*, const VulkanStoryFsr3Frame*);
int32_t rsf_core_fsr3_destroy(VulkanStoryFsr3Context*);

int32_t rsf_core_fsr3_swapchain_create(const rsf_fsr3fg_platform*, const rsf_fsr3wsi_create*, VulkanStoryFsr3Swapchain**);
VkSwapchainKHR rsf_core_fsr3_swapchain_handle(VulkanStoryFsr3Swapchain*);
VkResult rsf_core_fsr3_swapchain_recreate(VulkanStoryFsr3Swapchain*, const VkSwapchainCreateInfoKHR*, VkSwapchainKHR*);
VkResult rsf_core_fsr3_swapchain_get_images(VulkanStoryFsr3Swapchain*, VkSwapchainKHR, uint32_t*, VkImage*);
VkResult rsf_core_fsr3_swapchain_acquire(VulkanStoryFsr3Swapchain*, VkSwapchainKHR, uint64_t, VkSemaphore, VkFence, uint32_t*);
VkResult rsf_core_fsr3_swapchain_present(VulkanStoryFsr3Swapchain*, VkQueue, const VkPresentInfoKHR*);
int32_t rsf_core_fsr3_swapchain_prepare_destroy(VulkanStoryFsr3Swapchain*, VkSwapchainKHR);
int32_t rsf_core_fsr3_swapchain_destroy_chain(VulkanStoryFsr3Swapchain*, VkSwapchainKHR);
uint64_t rsf_core_fsr3_swapchain_last_present_count(VulkanStoryFsr3Swapchain*, VkSwapchainKHR);
int32_t rsf_core_fsr3_swapchain_destroy(VulkanStoryFsr3Swapchain*);
int32_t rsf_core_fsr3_fg_create(const rsf_fsr3fg_platform*, const rsf_fsr3fg_create*, VulkanStoryFsr3FgContext**);
int32_t rsf_core_fsr3_fg_disable(VulkanStoryFsr3FgContext*, VulkanStoryFsr3Swapchain*);
int32_t rsf_core_fsr3_fg_evaluate(VulkanStoryFsr3FgContext*, const VulkanStoryFsr3FgFrame*);
int32_t rsf_core_fsr3_fg_evaluate_direct(VulkanStoryFsr3FgContext*, const VulkanStoryFsr3FgFrame*);
int32_t rsf_core_fsr3_fg_destroy(VulkanStoryFsr3FgContext*);
}

static_assert(sizeof(rsf_fsr3_sdk) == 48 && offsetof(rsf_fsr3_sdk, dispatch) == 32);
static_assert(sizeof(rsf_fsr3sr_create) == 32 && offsetof(rsf_fsr3sr_create, render_width) == 16);
static_assert(sizeof(rsf_fsr3sr_platform) == 8 + 9 * sizeof(void*) &&
    offsetof(rsf_fsr3sr_platform, resolve) == 8 && offsetof(rsf_fsr3sr_platform, create) == 40 &&
    offsetof(rsf_fsr3sr_platform, destroy) == 72);
static_assert(sizeof(rsf_fsr3wsi_create) == 64 && offsetof(rsf_fsr3wsi_create, info) == 48 &&
    offsetof(rsf_fsr3wsi_create, queue_family) == 56);
static_assert(sizeof(rsf_fsr3fg_create) == 32 && offsetof(rsf_fsr3fg_create, back_buffer_format) == 24);
static_assert(sizeof(rsf_fsr3fg_platform) == 8 + 25 * sizeof(void*) &&
    offsetof(rsf_fsr3fg_platform, wsi_allocate) == 8 && offsetof(rsf_fsr3fg_platform, wsi_recreate) == 80 &&
    offsetof(rsf_fsr3fg_platform, fg_allocate) == 128 && offsetof(rsf_fsr3fg_platform, fg_generate) == 200);
