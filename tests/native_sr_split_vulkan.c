// SPDX-License-Identifier: GPL-3.0-only
#include <rescaleframe/native_sr_split_vulkan.h>
#include <stddef.h>
_Static_assert(RSF_SRVK_ABI_VERSION == 2, "SR ABI 2 appends sharpening");
_Static_assert(RSF_SRVK_SPLIT_ABI_VERSION == 1, "split ABI starts at 1");
_Static_assert(offsetof(rsf_fsr4vk_handoff, struct_size) == 0, "size leads");
_Static_assert(offsetof(rsf_fsr4vk_handoff, abi_version) == 4, "version follows");
_Static_assert(sizeof(rsf_fsr4vk_handoff) == 32, "Windows x64 handoff layout");
/* Real C declarations, including the callback and all DLL imports. */
int32_t rsf_test_split_c_declarations(const rsf_srvk_setup* setup,
    const rsf_fsr4vk_handoff* handoff, const rsf_srvk_frame* frame) {
    rsf_fsr4vk* owner = NULL;
    uint32_t width = 0, height = 0;
    void* continuation = NULL;
    int32_t result = rsf_srvk_fsr4_create(setup, handoff, &owner);
    if (!result) result = rsf_srvk_fsr4_plan(owner, &width, &height);
    if (!result) result = rsf_srvk_fsr4_evaluate(owner, NULL, frame, &continuation);
    if (owner) return rsf_srvk_fsr4_destroy(owner);
    return result;
}
