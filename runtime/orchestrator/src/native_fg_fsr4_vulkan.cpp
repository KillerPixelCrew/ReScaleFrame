// SPDX-License-Identifier: MIT
#include "native_fg_fsr4_vulkan_platform.h"

static_assert(sizeof(rsf_fsr4gvk_ops) == 43 * sizeof(void*) && sizeof(rsf_fsr4gvk_options) == 24 &&
    sizeof(rsf_fsr4fg_create) == 48 && sizeof(rsf_fsr4fg_info) == 32 &&
    sizeof(rsf_fsr4fg_frame) == 80 && sizeof(rsf_fsr4fg_present_status) == 32);

namespace {
int32_t interop_create(const rsf_srvk_setup* setup, rsf_fsr4fg_bridge* bridge, uint64_t luid,
    uint32_t timeline_enabled, rsf_dx12vk_interop** output) {
    const rsf_dx12vk_interop_callbacks callbacks{sizeof(callbacks), 1, luid,
        bridge, RsfFsr4FgCreateImage, RsfFsr4FgReleaseImage, RsfFsr4FgCreateFence};
    return rsf_dx12vk_interop_create(setup, &callbacks, timeline_enabled, output);
}
const rsf_fsr4gvk_ops operations{
    RsfFsr4FgProbe, RsfFsr4FgCreate, RsfFsr4FgInfo, RsfFsr4FgStart,
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
    // Rust admits the setup before it asks the embedded graphics table to describe create_info.
    return rsf_core_fsr4gvk_create(&operations, setup, options, output, chain);
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
