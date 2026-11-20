// SPDX-License-Identifier: MIT
#include <windows.h>
#include "native_fg_fsr4_vulkan_platform.h"

static_assert(sizeof(rsf_fsr4gvk_ops) == 37 * sizeof(void*) && sizeof(rsf_fsr4gvk_options) == 24 &&
    sizeof(rsf_fsr4fg_create) == 48 && sizeof(rsf_fsr4fg_info) == 32 &&
    sizeof(rsf_fsr4fg_frame) == 80 && sizeof(rsf_fsr4fg_present_status) == 32);

namespace {
int32_t directory(const char* path, uint16_t* output, int32_t capacity) {
    return MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, path, -1, reinterpret_cast<wchar_t*>(output), capacity);
}
int32_t interop_create(const rsf_srvk_setup* setup, rsf_fsr4fg_bridge* bridge, uint64_t luid,
    uint32_t timeline_enabled, rsf_dx12vk_interop** output) {
    const rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), 1, luid,
        bridge, RsfFsr4FgCreateImage, RsfFsr4FgReleaseImage, RsfFsr4FgCreateFence};
    return rsf_dx12vk_interop_create(setup, &callbacks, timeline_enabled, output);
}
const rsf_fsr4gvk_ops operations{
    directory, RsfFsr4FgProbe, RsfFsr4FgCreate, RsfFsr4FgInfo, RsfFsr4FgStart,
    RsfFsr4FgPresent, RsfFsr4FgDrain, RsfFsr4FgPrepareDestroy, RsfFsr4FgDestroy,
    interop_create, rsf_dx12gvk_interop,
    rsf_dx12gvk_graphics,
};
}
int32_t rsf_fsr4gvk_probe(const rsf_srvk_setup* setup, rsf_fsr4fg_info* out) {
    return rsf_core_fsr4gvk_probe(&operations, setup, out);
}
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options,
    rsf_fsr4gvk** output, uint64_t* chain) {
    rsf_fsr4gvk_swapchain swapchain{};
    const rsf_fsr4gvk_swapchain* description = nullptr;
    if (setup && setup->struct_size >= sizeof(*setup) && setup->abi_version == RSF_FGVK_ABI_VERSION && setup->create_info &&
        options && options->struct_size >= sizeof(*options) && options->abi_version == 1 && output && chain) {
        swapchain = rsf_dx12gvk_describe(setup->create_info);
        description = &swapchain;
    }
    return rsf_core_fsr4gvk_create(&operations, setup, options, description, output, chain);
}
namespace {
int32_t present(rsf_fsr4gvk* owner, const void* info, const rsf_frame_record* source, uint32_t explicit_source) {
    return rsf_dx12gvk_present_marshaled([](void* context, const auto* present_info, const auto* identity, uint32_t exact) {
        return rsf_core_fsr4gvk_present(static_cast<rsf_fsr4gvk*>(context), present_info, identity, exact);
    }, owner, info, source, explicit_source);
}
}
int32_t rsf_fsr4gvk_present(rsf_fsr4gvk* owner, const void* info) { return present(owner, info, nullptr, 0); }
int32_t rsf_fsr4gvk_present_source(rsf_fsr4gvk* owner, const void* info, const rsf_frame_record* source) {
    return present(owner, info, source, 1);
}

int32_t rsf_fsr4gvk_images(rsf_fsr4gvk* owner, uint32_t* count, uint64_t* output) {
    return rsf_core_fsr4gvk_images(owner, count, output);
}
int32_t rsf_fsr4gvk_acquire(rsf_fsr4gvk* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_fsr4gvk_acquire(owner, timeout, semaphore, fence, index);
}
int32_t rsf_fsr4gvk_prepare(rsf_fsr4gvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_fsr4gvk_prepare(owner, commands, frame);
}
int32_t rsf_fsr4gvk_hudless(rsf_fsr4gvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_fsr4gvk_hudless(owner, commands, frame);
}
void rsf_fsr4gvk_submitted(rsf_fsr4gvk* owner, void* const* commands, uint32_t count) {
    rsf_core_fsr4gvk_submitted(owner, commands, count);
}
int32_t rsf_fsr4gvk_enable(rsf_fsr4gvk* owner, uint32_t enabled) { return rsf_core_fsr4gvk_enable_options(owner, enabled, 1); }
int32_t rsf_fsr4gvk_enable_options(rsf_fsr4gvk* owner, uint32_t enabled, uint32_t count) {
    return rsf_core_fsr4gvk_enable_options(owner, enabled, count);
}
uint32_t rsf_fsr4gvk_max_generated(const rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_max_generated(owner); }
int32_t rsf_fsr4gvk_status(rsf_fsr4gvk* owner, rsf_fgvk_status* status) { return rsf_core_fsr4gvk_status(owner, status); }
int32_t rsf_fsr4gvk_drain(rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_drain(owner); }
int32_t rsf_fsr4gvk_destroy(rsf_fsr4gvk* owner) { return rsf_core_fsr4gvk_destroy(owner); }
