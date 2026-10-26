// SPDX-License-Identifier: GPL-3.0-only
#pragma once
#include <rescaleframe/native_fg_vulkan.h>

struct rsf_ffxgvk;
extern "C" {
rsf_backend_result rsf_ffxgvk_create(const rsf_fgvk_setup*, rsf_ffxgvk**, uint64_t* chain);
int32_t rsf_ffxgvk_images(rsf_ffxgvk*, uint32_t* count, uint64_t* images);
int32_t rsf_ffxgvk_acquire(rsf_ffxgvk*, uint64_t timeout, uint64_t semaphore, uint64_t fence, uint32_t* index);
rsf_backend_result rsf_ffxgvk_enable(rsf_ffxgvk*, uint32_t enabled);
rsf_backend_result rsf_ffxgvk_prepare(rsf_ffxgvk*, void* commands, const rsf_fgvk_frame*);
rsf_backend_result rsf_ffxgvk_hudless(rsf_ffxgvk*, void* commands, const rsf_fgvk_frame*);
void rsf_ffxgvk_submitted(rsf_ffxgvk*, void* const* commands, uint32_t count);
int32_t rsf_ffxgvk_present(rsf_ffxgvk*, const void* present_info);
rsf_backend_result rsf_ffxgvk_get_status(rsf_ffxgvk*, rsf_fgvk_status*);
rsf_backend_result rsf_ffxgvk_drain(rsf_ffxgvk*);
rsf_backend_result rsf_ffxgvk_destroy(rsf_ffxgvk*);
}
