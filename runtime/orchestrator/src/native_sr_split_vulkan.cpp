// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_split_vulkan.h>
#if defined(RSF_VULKAN_FSR4_ENABLED)
#include "native_sr_fsr4_vulkan.h"
#endif

#if defined(RSF_VULKAN_FSR4_ENABLED)
namespace {
int32_t public_result(int32_t result) {
    if (result == RSF_FSR4_BRIDGE_INVALID_ARGUMENT) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (result == RSF_FSR4_BRIDGE_ABI_MISMATCH) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    return result;
}
}
#endif

extern "C" int32_t rsf_srvk_fsr4_create(const rsf_srvk_setup* setup,
    const rsf_fsr4vk_handoff* handoff, rsf_fsr4vk** owner) try {
    if (!owner) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *owner = nullptr;
    if (!setup || setup->struct_size < sizeof(*setup) || !handoff || handoff->struct_size < sizeof(*handoff))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (setup->abi_version != RSF_SRVK_ABI_VERSION || handoff->abi_version != RSF_SRVK_SPLIT_ABI_VERSION)
        return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (setup->backend != RSF_SR_FSR4) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
    if (setup->quality > 4 || !setup->instance || !setup->physical_device || !setup->device ||
        !setup->instance_proc || !setup->device_proc || !setup->output_width || !setup->output_height ||
        !setup->runtime_directory_utf8 || !*setup->runtime_directory_utf8 || handoff->timeline_enabled != 1 ||
        handoff->queue_family >= 0xfffffffdu || !handoff->split_record ||
        setup->depth_inverted > 1 || setup->depth_infinite > 1 || setup->motion_jittered > 1)
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    return public_result(rsf_fsr4vk_create(setup, handoff, owner));
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" int32_t rsf_srvk_fsr4_plan(const rsf_fsr4vk* owner, uint32_t* width, uint32_t* height) try {
    if (width) *width = 0;
    if (height) *height = 0;
    if (!owner || !width || !height) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    return public_result(rsf_fsr4vk_plan(owner, width, height));
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" int32_t rsf_srvk_fsr4_evaluate(rsf_fsr4vk* owner, void* commands,
    const rsf_srvk_frame* frame, void** continuation) try {
    if (!continuation) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    *continuation = commands;
    if (!owner || !commands || !frame || frame->struct_size < sizeof(*frame))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->abi_version != RSF_SRVK_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (!frame->record || frame->record->struct_size < sizeof(*frame->record))
        return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->record->abi_version != RSF_GAME_FRAME_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (frame->record->camera.struct_size < sizeof(frame->record->camera)) return RSF_BACKEND_ERROR_INVALID_ARGUMENT;
    if (frame->record->camera.abi_version != RSF_GAME_FRAME_ABI_VERSION) return RSF_BACKEND_ERROR_ABI_MISMATCH;
    if (frame->reactive.image || frame->composition.image) return RSF_BACKEND_ERROR_NOT_SUPPORTED;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    return public_result(rsf_fsr4vk_evaluate(owner, commands, frame, continuation));
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" int32_t rsf_srvk_fsr4_destroy(rsf_fsr4vk* owner) try {
    if (!owner) return RSF_BACKEND_OK;
#if defined(RSF_VULKAN_FSR4_ENABLED)
    return public_result(rsf_fsr4vk_destroy(owner));
#else
    return RSF_BACKEND_ERROR_NOT_COMPILED;
#endif
} catch (...) { return RSF_BACKEND_ERROR_FEATURE_FAILED; }

extern "C" void rsf_srvk_fsr4_retire_commands(rsf_fsr4vk* self,void* commands) {
#if defined(RSF_VULKAN_FSR4_ENABLED)
    rsf_fsr4vk_retire_commands(self,commands);
#else
    (void)self; (void)commands;
#endif
}
