// SPDX-License-Identifier: MIT
#include <wchar.h>
#include "../../runtime/vulkan/fsr4-fg/bridge_api.h"

_Static_assert(sizeof(wchar_t) == sizeof(uint16_t), "expected Windows UTF-16 arguments");

int32_t rsf_fsr4_fg_bridge_fixture(int32_t, const uint16_t* const*,
    int32_t (*)(const rsf_fsr4fg_create*, rsf_fsr4fg_bridge**),
    int32_t (*)(uint64_t, const uint16_t*, rsf_fsr4fg_info*),
    int32_t (*)(rsf_fsr4fg_bridge*, void*),
    int32_t (*)(rsf_fsr4fg_bridge*, const rsf_fsr4fg_frame*, rsf_fsr4fg_present_status*));

int wmain(int argc, wchar_t** argv) {
    return rsf_fsr4_fg_bridge_fixture(argc, (const uint16_t* const*)argv,
        RsfFsr4FgCreate, RsfFsr4FgProbe, RsfFsr4FgStart, RsfFsr4FgPresent);
}
