// SPDX-License-Identifier: MIT
#include "native_fg_xess_vulkan_platform.h"

extern "C" int VulkanStoryXessFgSetEnabled(rsf_xess_bridge*, uint32_t);
extern "C" int VulkanStoryXessFgSleep(rsf_xess_bridge*, uint32_t);
extern "C" int VulkanStoryXessFgMarker(rsf_xess_bridge*, uint32_t, int);
extern "C" int VulkanStoryXessFgSetLatencyMode(rsf_xess_bridge*, uint32_t, uint32_t);
extern "C" int VulkanStoryXessFgWaitIdle(rsf_xess_bridge*);
extern "C" int VulkanStoryXessFgPrepareDestroy(rsf_xess_bridge*);
extern "C" int VulkanStoryXessFgDestroyChecked(rsf_xess_bridge*);

static_assert(sizeof(rsf_xessgvk_ops) == 17 * sizeof(void*) && sizeof(rsf_xessgvk_options) == 32 &&
    sizeof(rsf_xess_bridge_create) == 24 && sizeof(rsf_xess_bridge_start) == 32 &&
    sizeof(rsf_xess_bridge_frame) == 216 && sizeof(rsf_xess_bridge_present_status) == 24);

namespace {
const rsf_xessgvk_ops& operations() {
    static const rsf_xessgvk_ops value{
        rsf_dx12gvk_graphics(), &rsf_dx12gvk_interop, rsf_xessvk_interop_create,
        RsfXessBridgeCreate, RsfXessBridgeStart, RsfXessBridgeMaxGenerated, RsfXessBridgeSetGenerated,
        VulkanStoryXessFgSetEnabled, RsfXessBridgePresent, RsfXessBridgeCaptureSource, RsfXessBridgeCapturedCallbacks,
        VulkanStoryXessFgWaitIdle, VulkanStoryXessFgPrepareDestroy, VulkanStoryXessFgDestroyChecked,
        VulkanStoryXessFgSleep, VulkanStoryXessFgMarker, VulkanStoryXessFgSetLatencyMode,
    };
    return value;
}

int32_t present(void* owner, const rsf_dx12gvk_present_info* info, const rsf_frame_record* source, uint32_t exact) {
    return rsf_core_xessgvk_present(static_cast<rsf_xessgvk*>(owner), info, source, exact);
}
}

int32_t rsf_xessgvk_create(const rsf_fgvk_setup* setup, const rsf_xessgvk_options* options,
    rsf_xessgvk** output, uint64_t* chain) {
    // Rust admits the setup before it asks the shared graphics table to describe create_info.
    return rsf_core_xessgvk_create(&operations(), setup, options, output, chain);
}

int32_t rsf_xessgvk_images(rsf_xessgvk* owner, uint32_t* count, uint64_t* output) {
    return rsf_core_xessgvk_images(owner, count, output);
}
int32_t rsf_xessgvk_acquire(rsf_xessgvk* owner, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index) {
    return rsf_core_xessgvk_acquire(owner, timeout, semaphore, fence, index);
}
int32_t rsf_xessgvk_prepare(rsf_xessgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_xessgvk_prepare(owner, commands, frame);
}
int32_t rsf_xessgvk_hudless(rsf_xessgvk* owner, void* commands, const rsf_fgvk_frame* frame) {
    return rsf_core_xessgvk_hudless(owner, commands, frame);
}
void rsf_xessgvk_submitted(rsf_xessgvk* owner, void* const* commands, uint32_t count) {
    rsf_core_xessgvk_submitted(owner, commands, count);
}
int32_t rsf_xessgvk_present(rsf_xessgvk* owner, const void* info) {
    return rsf_dx12gvk_present_marshaled(present, owner, info, nullptr, 0);
}
int32_t rsf_xessgvk_present_source(rsf_xessgvk* owner, const void* info, const rsf_frame_record* source) {
    return rsf_dx12gvk_present_marshaled(present, owner, info, source, 1);
}
int32_t rsf_xessgvk_enable(rsf_xessgvk* owner, uint32_t enabled) {
    return rsf_core_xessgvk_enable(owner, enabled);
}
int32_t rsf_xessgvk_enable_options(rsf_xessgvk* owner, uint32_t enabled, uint32_t count) {
    return rsf_core_xessgvk_enable_options(owner, enabled, count);
}
uint32_t rsf_xessgvk_max_generated(const rsf_xessgvk* owner) {
    return rsf_core_xessgvk_max_generated(owner);
}
int32_t rsf_xessgvk_sleep(rsf_xessgvk* owner, uint32_t frame) {
    return rsf_core_xessgvk_sleep(owner, frame);
}
int32_t rsf_xessgvk_latency(rsf_xessgvk* owner, uint32_t frame, uint32_t marker) {
    return rsf_core_xessgvk_latency(owner, frame, marker);
}
int32_t rsf_xessgvk_latency_options(rsf_xessgvk* owner, uint32_t mode, uint32_t minimum_interval_us) {
    return rsf_core_xessgvk_latency_options(owner, mode, minimum_interval_us);
}
int32_t rsf_xessgvk_latency_status(rsf_xessgvk* owner, rsf_fgvk_latency_status* status) {
    return rsf_core_xessgvk_latency_status(owner, status);
}
int32_t rsf_xessgvk_status(rsf_xessgvk* owner, rsf_fgvk_status* status) {
    return rsf_core_xessgvk_status(owner, status);
}
int32_t rsf_xessgvk_drain(rsf_xessgvk* owner) {
    return rsf_core_xessgvk_drain(owner);
}
int32_t rsf_xessgvk_destroy(rsf_xessgvk* owner) {
    return rsf_core_xessgvk_destroy(owner);
}
