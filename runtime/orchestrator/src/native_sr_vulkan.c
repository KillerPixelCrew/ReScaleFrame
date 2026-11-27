/* SPDX-License-Identifier: MIT */
/* FidelityFX Vulkan SR SDK layout checks. Rust (vulkan_sr.rs and vulkan_sr/graphics.rs) owns the
   platform table, the SDK calls and their foreign-exception guard, and builds the SDK records
   from the pinned FidelityFX 1.1.4 Vulkan bindings (runtime/vulkan/fsr3/sdk_abi.rs). This unit
   checks the configured SDK headers against those records and values. */
#if defined(RSF_HAVE_FFX_VK)
#include <stdbool.h>
#include <stddef.h>
#include <vulkan/vulkan.h>
#include <ffx_api/ffx_api.h>
#include <ffx_api/ffx_upscale.h>
/* ffx_api_vk.h names these structs without the struct keyword, as C++ allows. */
typedef struct FfxApiResource FfxApiResource;
typedef struct FfxApiResourceDescription FfxApiResourceDescription;
typedef struct VkQueueInfoFFXAPI VkQueueInfoFFXAPI;
#include <ffx_api/vk/ffx_api_vk.h>

typedef struct ffxQueryDescUpscaleGetRenderResolutionFromQualityMode rsf_srvk_plan_desc;
typedef struct ffxDispatchDescUpscale rsf_srvk_dispatch_desc;

#define RSF_ASSERT(condition) _Static_assert(condition, #condition)
#define RSF_FFX_FIELD(type, field, offset) RSF_ASSERT(offsetof(type, field) == offset)
RSF_ASSERT(sizeof(ffxApiHeader) == 16 && _Alignof(ffxApiHeader) == 8);
RSF_FFX_FIELD(ffxApiHeader, type, 0);
RSF_FFX_FIELD(ffxApiHeader, pNext, 8);
RSF_ASSERT(sizeof(struct FfxApiDimensions2D) == 8 && sizeof(struct FfxApiFloatCoords2D) == 8);
RSF_FFX_FIELD(struct FfxApiDimensions2D, width, 0);
RSF_FFX_FIELD(struct FfxApiDimensions2D, height, 4);
RSF_FFX_FIELD(struct FfxApiFloatCoords2D, x, 0);
RSF_FFX_FIELD(struct FfxApiFloatCoords2D, y, 4);
RSF_ASSERT(sizeof(struct ffxCreateBackendVKDesc) == 40 && _Alignof(struct ffxCreateBackendVKDesc) == 8);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, header, 0);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkDevice, 16);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkPhysicalDevice, 24);
RSF_FFX_FIELD(struct ffxCreateBackendVKDesc, vkDeviceProcAddr, 32);
RSF_ASSERT(sizeof(struct ffxCreateContextDescUpscale) == 48 && _Alignof(struct ffxCreateContextDescUpscale) == 8);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, header, 0);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, flags, 16);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, maxRenderSize, 20);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, maxUpscaleSize, 28);
RSF_FFX_FIELD(struct ffxCreateContextDescUpscale, fpMessage, 40);
RSF_ASSERT(sizeof(rsf_srvk_plan_desc) == 48 && _Alignof(rsf_srvk_plan_desc) == 8);
RSF_FFX_FIELD(rsf_srvk_plan_desc, header, 0);
RSF_FFX_FIELD(rsf_srvk_plan_desc, displayWidth, 16);
RSF_FFX_FIELD(rsf_srvk_plan_desc, displayHeight, 20);
RSF_FFX_FIELD(rsf_srvk_plan_desc, qualityMode, 24);
RSF_FFX_FIELD(rsf_srvk_plan_desc, pOutRenderWidth, 32);
RSF_FFX_FIELD(rsf_srvk_plan_desc, pOutRenderHeight, 40);
RSF_ASSERT(sizeof(struct FfxApiResourceDescription) == 32 && _Alignof(struct FfxApiResourceDescription) == 4);
RSF_FFX_FIELD(struct FfxApiResourceDescription, type, 0);
RSF_FFX_FIELD(struct FfxApiResourceDescription, format, 4);
RSF_FFX_FIELD(struct FfxApiResourceDescription, width, 8);
RSF_FFX_FIELD(struct FfxApiResourceDescription, height, 12);
RSF_FFX_FIELD(struct FfxApiResourceDescription, depth, 16);
RSF_FFX_FIELD(struct FfxApiResourceDescription, mipCount, 20);
RSF_FFX_FIELD(struct FfxApiResourceDescription, flags, 24);
RSF_FFX_FIELD(struct FfxApiResourceDescription, usage, 28);
RSF_ASSERT(sizeof(struct FfxApiResource) == 48 && _Alignof(struct FfxApiResource) == 8);
RSF_FFX_FIELD(struct FfxApiResource, resource, 0);
RSF_FFX_FIELD(struct FfxApiResource, description, 8);
RSF_FFX_FIELD(struct FfxApiResource, state, 40);
RSF_ASSERT(sizeof(rsf_srvk_dispatch_desc) == 432 && _Alignof(rsf_srvk_dispatch_desc) == 8);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, header, 0);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, commandList, 16);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, color, 24);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, depth, 72);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, motionVectors, 120);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, exposure, 168);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, reactive, 216);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, transparencyAndComposition, 264);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, output, 312);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, jitterOffset, 360);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, motionVectorScale, 368);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, renderSize, 376);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, upscaleSize, 384);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, enableSharpening, 392);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, sharpness, 396);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, frameTimeDelta, 400);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, preExposure, 404);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, reset, 408);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraNear, 412);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraFar, 416);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, cameraFovAngleVertical, 420);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, viewSpaceToMetersFactor, 424);
RSF_FFX_FIELD(rsf_srvk_dispatch_desc, flags, 428);
RSF_ASSERT(FFX_API_CREATE_CONTEXT_DESC_TYPE_BACKEND_VK == 3 &&
    FFX_API_CREATE_CONTEXT_DESC_TYPE_UPSCALE == 65536 &&
    FFX_API_DISPATCH_DESC_TYPE_UPSCALE == 65537 &&
    FFX_API_QUERY_DESC_TYPE_UPSCALE_GETRENDERRESOLUTIONFROMQUALITYMODE == 65539);
RSF_ASSERT(FFX_API_SURFACE_FORMAT_UNKNOWN == 0 && FFX_API_SURFACE_FORMAT_R16G16B16A16_FLOAT == 4 &&
    FFX_API_SURFACE_FORMAT_R16G16_FLOAT == 18 && FFX_API_SURFACE_FORMAT_R8_UNORM == 25 &&
    FFX_API_SURFACE_FORMAT_R32_FLOAT == 28);
RSF_ASSERT(FFX_API_RESOURCE_TYPE_TEXTURE2D == 2 && FFX_API_RESOURCE_USAGE_READ_ONLY == 0 &&
    FFX_API_RESOURCE_USAGE_UAV == 2 && FFX_API_RESOURCE_USAGE_DEPTHTARGET == 4 &&
    FFX_API_RESOURCE_STATE_UNORDERED_ACCESS == 2 && FFX_API_RESOURCE_STATE_COMPUTE_READ == 4);
RSF_ASSERT(FFX_UPSCALE_ENABLE_HIGH_DYNAMIC_RANGE == 1 && FFX_UPSCALE_ENABLE_MOTION_VECTORS_JITTER_CANCELLATION == 4 &&
    FFX_UPSCALE_ENABLE_DEPTH_INVERTED == 8 && FFX_UPSCALE_ENABLE_DEPTH_INFINITE == 16 &&
    FFX_UPSCALE_ENABLE_AUTO_EXPOSURE == 32);
/* graphics.rs declares these two ffx_api.h records itself. */
RSF_ASSERT(sizeof(struct ffxQueryDescGetVersions) == 56 && sizeof(struct ffxOverrideVersion) == 24);
#endif