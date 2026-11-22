/* SPDX-License-Identifier: MIT */
/* Production Streamline table of the Rust Vulkan DLSS SR owner. The SDK,
   device, image, sharpening and capture leaves are Rust
   (vulkan_dlss_sr/leaves.rs); this table adds the session invoke, the
   FrameDump parameter record and the SDK preset policy shared with D3D. */
#include "native_dlss_vulkan_platform.h"

#if defined(RSF_HAVE_STREAMLINE_VK)
#include "../../backends/dlss/include/rescaleframe/dlss_preset.h"
#include <rescaleframe/capture_names.h>
#include <rescaleframe/sr_session.h>

int32_t rsf_core_vkdlss_sdk_open(const rsf_srvk_setup*, void**);
void rsf_core_vkdlss_sdk_close(void*);
rsf_backend_result rsf_core_vkdlss_sdk_initialize(void*, uintptr_t, void*);
int32_t rsf_core_vkdlss_sdk_prepare(void*);
uint32_t rsf_core_vkdlss_sdk_allocate(void*, rsf_vkfg_image*, uint32_t, uint32_t, uint32_t, uint32_t);
void rsf_core_vkdlss_sdk_free_image(void*, rsf_vkfg_image*);
int32_t rsf_core_vkdlss_sdk_constants(void*, uintptr_t, const rsf_slvk_sdk_constants*);
int32_t rsf_core_vkdlss_sdk_options(void*, const rsf_vkdlss_options*);
int32_t rsf_core_vkdlss_sdk_tag(void*, uintptr_t, void*, const rsf_vkdlss_tag*);
int32_t rsf_core_vkdlss_sdk_evaluate(void*, uintptr_t, void*);
void rsf_core_vkdlss_sdk_capture_record(void*, void*, const rsf_srvk_image*, const rsf_frame_record*,
                                        const char*, uint32_t);
void rsf_core_vkdlss_sdk_sharpen(void*, void*, const rsf_srvk_frame*, uint64_t);
int32_t rsf_core_vkdlss_sdk_release(void*, uintptr_t, void*);
void rsf_core_vkdlss_sdk_flush(void*);
void rsf_core_vkdlss_sdk_retire(void*, void*);

static void capture_parameters(void* native, const rsf_srvk_frame* input, uint32_t reset) {
    (void)native;
    rsf_capture_sr_parameters(input->record, RSF_SR_DLSS, input->motion_scale_x, input->motion_scale_y,
                              input->pre_exposure, reset);
}

static int32_t preset_valid(uint32_t preset) {
    return rsf_dlss_preset_valid(preset);
}

const rsf_vkdlss_platform rsf_vkdlss_native_leaves = {
    rsf_core_vkdlss_sdk_open,
    rsf_core_vkdlss_sdk_close,
    rsf_slvk_invoke,
    rsf_core_vkdlss_sdk_initialize,
    rsf_core_vkdlss_sdk_prepare,
    rsf_core_vkdlss_sdk_allocate,
    rsf_core_vkdlss_sdk_free_image,
    rsf_core_vkdlss_sdk_constants,
    rsf_core_vkdlss_sdk_options,
    rsf_core_vkdlss_sdk_tag,
    rsf_core_vkdlss_sdk_evaluate,
    capture_parameters,
    rsf_core_vkdlss_sdk_capture_record,
    rsf_core_vkdlss_sdk_sharpen,
    rsf_core_vkdlss_sdk_release,
    rsf_core_vkdlss_sdk_flush,
    rsf_core_vkdlss_sdk_retire,
    preset_valid,
};

_Static_assert(sizeof(rsf_vkdlss_platform) == 18 * sizeof(void*), "DLSS SR platform");
_Static_assert(sizeof(rsf_vkdlss_tag) == 24, "DLSS SR tag plan");
#endif
