/* SPDX-License-Identifier: MIT */
/* Public rsf_dlssgvk exports of the Rust DLSS-G owner (vulkan_dlss_fg.rs) and
   its production platform table. The Streamline, Vulkan and capture leaves are
   Rust (vulkan_dlss_fg/leaves.rs); this table adds the session invoke and the
   worker timeout epoch of the shared Streamline lifecycle. */
#include "native_fg_dlss_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
uint64_t rsf_vk_sl_worker_timeout_epoch(void);
rsf_backend_result rsf_core_dlgvk_sdk_open(const rsf_fgvk_setup*, void**);
void rsf_core_dlgvk_sdk_close(void*);
rsf_backend_result rsf_core_dlgvk_sdk_prepare_chain(void*, uint32_t*, void**);
rsf_backend_result rsf_core_dlgvk_sdk_start_capture(void*);
rsf_backend_result rsf_core_dlgvk_sdk_initialize(void*, uintptr_t, void*);
rsf_backend_result rsf_core_dlgvk_sdk_configure(void*, uintptr_t, void*);
rsf_backend_result rsf_core_dlgvk_sdk_sample(void*, uintptr_t, void*);
rsf_backend_result rsf_core_dlgvk_sdk_constants(void*, uintptr_t, void*);
rsf_backend_result rsf_core_dlgvk_sdk_tag(void*, uintptr_t, void*);
rsf_backend_result rsf_core_dlgvk_sdk_release(void*, uintptr_t, void*);
void rsf_core_dlgvk_sdk_capture(void*, void*, const rsf_srvk_image*, const rsf_frame_record*, uint32_t, uint64_t);
void rsf_core_dlgvk_sdk_remember(void*, const rsf_frame_record*, const rsf_dlssgvk_status*, uint32_t);
void rsf_core_dlgvk_sdk_capture_group(void*, uint64_t, uint32_t);
void rsf_core_dlgvk_sdk_flush(void*);
rsf_backend_result rsf_core_dlgvk_sdk_detach_capture(void*);

const rsf_dlgvk_platform rsf_dlgvk_leaves = {
    &rsf_core_vkfg_platform,
    rsf_core_dlgvk_sdk_open,
    rsf_core_dlgvk_sdk_close,
    rsf_core_dlgvk_sdk_prepare_chain,
    rsf_core_dlgvk_sdk_start_capture,
    rsf_slvk_invoke,
    rsf_core_dlgvk_sdk_initialize,
    rsf_core_dlgvk_sdk_configure,
    rsf_core_dlgvk_sdk_sample,
    rsf_core_dlgvk_sdk_constants,
    rsf_core_dlgvk_sdk_tag,
    rsf_core_dlgvk_sdk_release,
    rsf_core_dlgvk_sdk_capture,
    rsf_core_dlgvk_sdk_remember,
    rsf_core_dlgvk_sdk_capture_group,
    rsf_core_dlgvk_sdk_flush,
    rsf_core_dlgvk_sdk_detach_capture,
    rsf_vk_sl_worker_timeout_epoch,
};
#define RSF_DLGVK_PLATFORM (&rsf_dlgvk_leaves)
#else
#define RSF_DLGVK_PLATFORM ((const rsf_dlgvk_platform*)0)
#endif

_Static_assert(sizeof(rsf_dlgvk_sample) == 40, "DLSS-G sample record");
_Static_assert(sizeof(rsf_dlgvk_tag) == 24, "DLSS-G tag plan");
_Static_assert(sizeof(rsf_dlgvk_sdk_call) == 96, "DLSS-G SDK call");
_Static_assert(sizeof(rsf_dlgvk_sdk) == 5 * sizeof(void*), "DLSS-G SDK entries");
_Static_assert(sizeof(rsf_dlgvk_platform) == 144, "DLSS-G platform");

rsf_backend_result rsf_dlssgvk_create(const rsf_fgvk_setup* setup, rsf_dlssgvk** out, uint64_t* chain) {
    return rsf_core_dlgvk_create(RSF_DLGVK_PLATFORM, setup, out, chain);
}
int32_t rsf_dlssgvk_images(rsf_dlssgvk* owner, uint32_t* count, uint64_t* images) {
    return rsf_core_dlgvk_images(owner, count, images);
}
int32_t rsf_dlssgvk_acquire(rsf_dlssgvk* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_dlgvk_acquire(owner, timeout, semaphore, fence, index);
}
rsf_backend_result rsf_dlssgvk_image_layout(const rsf_dlssgvk* owner, uint32_t* layout) {
    return rsf_core_dlgvk_image_layout(owner, layout);
}
rsf_backend_result rsf_dlssgvk_enable(rsf_dlssgvk* owner, uint32_t enabled, uint32_t count) {
    return rsf_core_dlgvk_enable(owner, enabled, count);
}
rsf_backend_result rsf_dlssgvk_prepare(rsf_dlssgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_dlgvk_record(owner, commands, frame, 0);
}
rsf_backend_result rsf_dlssgvk_hudless(rsf_dlssgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_dlgvk_record(owner, commands, frame, 1);
}
void rsf_dlssgvk_submitted(rsf_dlssgvk* owner, void* const* commands, uint32_t count) {
    rsf_core_dlgvk_submitted(owner, commands, count);
}
int32_t rsf_dlssgvk_present(rsf_dlssgvk* owner, const void* info) {
    return rsf_core_dlgvk_present(owner, info, 0, 0);
}
int32_t rsf_dlssgvk_present_source(rsf_dlssgvk* owner, const void* info, const rsf_frame_record* source) {
    return rsf_core_dlgvk_present(owner, info, source, 1);
}
rsf_backend_result rsf_dlssgvk_get_status(rsf_dlssgvk* owner, rsf_dlssgvk_status* status) {
    return rsf_core_dlgvk_status(owner, status);
}
rsf_backend_result rsf_dlssgvk_drain(rsf_dlssgvk* owner) {
    return rsf_core_dlgvk_drain(owner);
}
rsf_backend_result rsf_dlssgvk_destroy(rsf_dlssgvk* owner) {
    return rsf_core_dlgvk_destroy(owner);
}
