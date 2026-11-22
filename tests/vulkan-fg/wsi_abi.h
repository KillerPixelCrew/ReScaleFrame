/* SPDX-License-Identifier: MIT */
#pragma once
#include <stdint.h>

typedef struct rsf_wsi_fixture_counts {
    uint32_t native_creates, native_destroys, host_creates, host_destroys;
    uint32_t native_presents, host_presents, host_acquires, host_images;
} rsf_wsi_fixture_counts;

typedef struct rsf_wsi_fixture_calls {
    int32_t (*registration)(uint32_t device, uint32_t abi, uint32_t mask, uint32_t short_record);
    int32_t (*create)(uint32_t device, uint64_t* chain);
    void (*destroy)(uint32_t device, uint64_t chain);
    int32_t (*present)(uint32_t device);
    int32_t (*acquire)(uint32_t device, uint64_t chain);
    int32_t (*images)(uint32_t device, uint64_t chain);
    void (*counts)(rsf_wsi_fixture_counts*);
} rsf_wsi_fixture_calls;

#ifdef __cplusplus
extern "C" {
#endif
int32_t rsf_ffx_vulkan_wsi_dispatch_fixture(const rsf_wsi_fixture_calls*);
#ifdef __cplusplus
}
#endif
