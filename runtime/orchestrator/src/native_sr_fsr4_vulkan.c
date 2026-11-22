/* SPDX-License-Identifier: MIT */
/* Platform table of the private FSR 4 Vulkan SR service. Rust owns the graphics capsule,
   barrier and copy recording, capture, sharpening and retirement (vulkan_fsr4_sr.rs) and the
   bridge itself (runtime/vulkan/fsr4). This leaf contains C++ exceptions from the host
   procedure resolvers, the host split callback and the bridge calls that reach the FidelityFX
   SDK. MSVC raises a C++ exception as SEH code 0xE06D7363, so a filter on that code stops the
   same exceptions the former catch-all stopped; other faults still propagate. */
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <vulkan/vulkan.h>
#include <stddef.h>
#include "native_sr_fsr4_vulkan_platform.h"

#define RSF_CXX_EXCEPTION_CODE 0xE06D7363UL

#if defined(_MSC_VER)
#define RSF_FOREIGN_TRY __try
#define RSF_FOREIGN_CATCH(on_exception) \
    __except (GetExceptionCode() == RSF_CXX_EXCEPTION_CODE ? EXCEPTION_EXECUTE_HANDLER \
                                                           : EXCEPTION_CONTINUE_SEARCH) { on_exception; }
#else
#error "Foreign C++ exception containment needs MSVC structured exception handling"
#endif

/* Bridge entry points defined by the Rust FSR 4 bridge (runtime/vulkan/fsr4/native.rs). */
int rsf_vk_fsr4_create_shared_image(rsf_fsr4_bridge*, uint32_t, uint32_t, uint32_t, uint32_t, void**, void**);
int rsf_vk_fsr4_create_shared_fence(rsf_fsr4_bridge*, void**);
int rsf_vk_fsr4_prepare_destroy(rsf_fsr4_bridge*);
int rsf_vk_fsr4_destroy_checked(rsf_fsr4_bridge*);
int32_t rsf_core_fsr4sr_info(const rsf_fsr4_bridge*, rsf_fsr4_bridge_info*);
int32_t rsf_core_fsr4sr_sharpness(rsf_fsr4_bridge*, float);
void rsf_core_fsr4sr_release_image(void*);
int32_t rsf_core_fsr4vk_create_admitted(const rsf_fsr4vk_platform*, const rsf_srvk_setup*,
    const rsf_fsr4vk_handoff*, rsf_fsr4vk**);

_Static_assert(sizeof(rsf_fsr4_bridge_create) == 56 && offsetof(rsf_fsr4_bridge_create, output_width) == 32,
    "bridge create layout");
_Static_assert(sizeof(rsf_fsr4_bridge_frame) == 112 && offsetof(rsf_fsr4_bridge_frame, reset) == 92 &&
    offsetof(rsf_fsr4_bridge_frame, done_value) == 104, "bridge frame layout");
_Static_assert(sizeof(rsf_fsr4_bridge_info) == sizeof(rsf_srvk_fsr4_provider_info) &&
    offsetof(rsf_fsr4_bridge_info, provider_version) == offsetof(rsf_srvk_fsr4_provider_info, provider_version),
    "bridge info matches the public provider info");
_Static_assert(VK_FORMAT_R16G16B16A16_SFLOAT == 97 && VK_FORMAT_R32_SFLOAT == 100 && VK_FORMAT_R16G16_SFLOAT == 83,
    "Vulkan formats used by vulkan_fsr4_sr.rs");
_Static_assert(RSF_FSR4_BRIDGE_ABI_VERSION == 1 && VK_NOT_READY == 1 && VK_ERROR_OUT_OF_HOST_MEMORY == -1 &&
    VK_ERROR_INITIALIZATION_FAILED == -3 && VK_ERROR_UNKNOWN == -13, "Vulkan results used by vulkan_fsr4_sr.rs");
_Static_assert(VK_IMAGE_LAYOUT_UNDEFINED == 0 && VK_IMAGE_LAYOUT_GENERAL == 1 &&
    VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL == 5 && VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL == 6 &&
    VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL == 7 && VK_IMAGE_USAGE_SAMPLED_BIT == 4 && VK_IMAGE_USAGE_STORAGE_BIT == 8,
    "Vulkan layouts and usages used by vulkan_fsr4_sr.rs");

static int32_t open_graphics(const rsf_srvk_setup* setup, void** out)
{
    RSF_FOREIGN_TRY {
        const rsf_vk_function entries[5] = {
            setup->instance_proc(setup->instance, "vkGetPhysicalDeviceMemoryProperties"),
            setup->device_proc(setup->device, "vkCmdPipelineBarrier"),
            setup->device_proc(setup->device, "vkCmdCopyImage"),
            setup->device_proc(setup->device, "vkWaitSemaphores"),
            setup->device_proc(setup->device, "vkDeviceWaitIdle"),
        };
        return rsf_fsr4vk_graphics_open(setup, &entries, out);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t bridge_create(const rsf_fsr4_bridge_create* setup, rsf_fsr4_bridge** out)
{
    RSF_FOREIGN_TRY {
        return RsfFsr4BridgeCreate(setup, out);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t bridge_plan(rsf_fsr4_bridge* bridge, uint32_t quality, uint32_t* width, uint32_t* height)
{
    RSF_FOREIGN_TRY {
        return RsfFsr4BridgePlan(bridge, quality, width, height);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t create_fence(void* bridge, void** handle)
{
    RSF_FOREIGN_TRY {
        return rsf_vk_fsr4_create_shared_fence((rsf_fsr4_bridge*)bridge, handle);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

/* The resource output is written on every path, including a contained exception. */
static int32_t create_image(rsf_fsr4_bridge* bridge, uint32_t width, uint32_t height, uint32_t format,
    uint32_t writable, void** handle, void** resource)
{
    void* image = NULL;
    int32_t result;
    RSF_FOREIGN_TRY {
        result = rsf_vk_fsr4_create_shared_image(bridge, width, height, format, writable, handle, &image);
    } RSF_FOREIGN_CATCH(result = RSF_BACKEND_ERROR_FEATURE_FAILED)
    *resource = image;
    return result;
}

static int32_t split(const rsf_fsr4vk_handoff* handoff, void* commands, uint64_t timeline,
    uint64_t previous, uint64_t ready, uint64_t done, void** continuation)
{
    RSF_FOREIGN_TRY {
        return handoff->split_record(handoff->user, commands, timeline, previous, ready, done, continuation);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t bridge_evaluate(rsf_fsr4_bridge* bridge, const rsf_fsr4_bridge_frame* frame)
{
    RSF_FOREIGN_TRY {
        return RsfFsr4BridgeEvaluate(bridge, frame);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t bridge_prepare_destroy(rsf_fsr4_bridge* bridge)
{
    RSF_FOREIGN_TRY {
        return rsf_vk_fsr4_prepare_destroy(bridge);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

static int32_t bridge_destroy(rsf_fsr4_bridge* bridge)
{
    RSF_FOREIGN_TRY {
        return rsf_vk_fsr4_destroy_checked(bridge);
    } RSF_FOREIGN_CATCH(return RSF_BACKEND_ERROR_FEATURE_FAILED)
}

/* The interop table is a Rust address, so it is stored once before first use. */
static rsf_fsr4vk_platform platform_table = {
    .interop = NULL,
    .open = open_graphics,
    .close = rsf_fsr4vk_graphics_close,
    .bridge_create = bridge_create,
    .bridge_plan = bridge_plan,
    .bridge_info = rsf_core_fsr4sr_info,
    .create_fence = create_fence,
    .create_image = create_image,
    .release_image = rsf_core_fsr4sr_release_image,
    .capture_initialize = rsf_fsr4vk_capture_initialize,
    .capture_begin = rsf_fsr4vk_capture_begin,
    .capture_record = rsf_fsr4vk_capture_record,
    .capture_parameters = rsf_fsr4vk_capture_parameters,
    .capture_flush = rsf_fsr4vk_capture_flush,
    .transition = rsf_fsr4vk_graphics_transition,
    .copy = rsf_fsr4vk_graphics_copy,
    .split = split,
    .sharpening_valid = NULL,
    .bridge_sharpness = rsf_core_fsr4sr_sharpness,
    .bridge_evaluate = bridge_evaluate,
    .sharpen = rsf_fsr4vk_graphics_sharpen,
    .retire = rsf_fsr4vk_graphics_retire,
    .clear_sharpen = rsf_fsr4vk_graphics_clear_sharpen,
    .wait = rsf_fsr4vk_graphics_wait,
    .idle = rsf_fsr4vk_graphics_idle,
    .bridge_prepare_destroy = bridge_prepare_destroy,
    .bridge_destroy = bridge_destroy,
};
static INIT_ONCE platform_once = INIT_ONCE_STATIC_INIT;

static BOOL CALLBACK bind_interop(PINIT_ONCE once, PVOID parameter, PVOID* context)
{
    (void)once;
    (void)parameter;
    (void)context;
    platform_table.interop = rsf_dx12vk_platform_ops();
    return TRUE;
}

static const rsf_fsr4vk_platform* platform(void)
{
    (void)InitOnceExecuteOnce(&platform_once, bind_interop, NULL, NULL);
    return &platform_table;
}

int32_t rsf_fsr4vk_create(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out)
{
    return rsf_core_fsr4vk_create(platform(), setup, handoff, out);
}

int32_t rsf_fsr4vk_create_admitted(const rsf_srvk_setup* setup, const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** out)
{
    return rsf_core_fsr4vk_create_admitted(platform(), setup, handoff, out);
}
