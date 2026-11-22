// SPDX-License-Identifier: MIT
#include "native_fg_fsr4_vulkan_platform.h"

static_assert(sizeof(rsf_fsr4gvk_ops) == 43 * sizeof(void*) && sizeof(rsf_fsr4gvk_options) == 24 &&
    sizeof(rsf_fsr4fg_create) == 48 && sizeof(rsf_fsr4fg_info) == 32 &&
    sizeof(rsf_fsr4fg_frame) == 80 && sizeof(rsf_fsr4fg_present_status) == 32);

namespace {
const rsf_fsr4gvk_ops& operations() {
    static const rsf_fsr4gvk_ops value{
        RsfFsr4FgProbe, RsfFsr4FgCreate, RsfFsr4FgInfo, RsfFsr4FgStart,
        RsfFsr4FgPresent, RsfFsr4FgDrain, RsfFsr4FgPrepareDestroy, RsfFsr4FgDestroy,
        rsf_fsr4gvk_interop_create, rsf_dx12gvk_interop, *rsf_dx12gvk_graphics(),
    };
    return value;
}
}
int32_t rsf_fsr4gvk_probe(const rsf_srvk_setup* setup, rsf_fsr4fg_info* out) {
    return rsf_core_fsr4gvk_probe(&operations(), setup, out);
}
int32_t rsf_fsr4gvk_create(const rsf_fgvk_setup* setup, const rsf_fsr4gvk_options* options,
    rsf_fsr4gvk** output, uint64_t* chain) {
    // Rust admits the setup before it asks the embedded graphics table to describe create_info.
    return rsf_core_fsr4gvk_create(&operations(), setup, options, output, chain);
}
