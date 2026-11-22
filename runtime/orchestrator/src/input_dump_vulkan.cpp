// SPDX-License-Identifier: MIT
#include <rescaleframe/input_dump_vulkan.h>
#include <cstddef>

static_assert(sizeof(rsf_input_dump_vk_setup) == 128 && offsetof(rsf_input_dump_vk_setup, submit) == 112);

extern "C" int32_t rsf_core_input_dump_vk_create(const rsf_input_dump_vk_setup*, rsf_input_dump_vk**);
extern "C" int32_t rsf_core_input_dump_vk_snapshot(rsf_input_dump_vk*, const rsf_frame_record*, const rsf_srvk_image*, const char*);
extern "C" int32_t rsf_core_input_dump_vk_drain(rsf_input_dump_vk*);
extern "C" int32_t rsf_core_input_dump_vk_destroy(rsf_input_dump_vk*);

extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_create(const rsf_input_dump_vk_setup* setup, rsf_input_dump_vk** out) {
    return rsf_core_input_dump_vk_create(setup, out);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_snapshot(rsf_input_dump_vk* owner, const rsf_frame_record* frame,
    const rsf_srvk_image* image, const char* name) {
    return rsf_core_input_dump_vk_snapshot(owner, frame, image, name);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_drain(rsf_input_dump_vk* owner) {
    return rsf_core_input_dump_vk_drain(owner);
}
extern "C" RSF_RUNTIME_API int32_t rsf_input_dump_vk_destroy(rsf_input_dump_vk* owner) {
    return rsf_core_input_dump_vk_destroy(owner);
}
