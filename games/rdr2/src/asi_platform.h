/* SPDX-License-Identifier: MIT */
#pragma once
#include "runtime_setup.h"
#include <stdint.h>

typedef struct rsf_rdr2_asi_leaves {
    uint32_t (*adapter_vendor)(uint32_t);
    int32_t (*d3d12_trace)(uintptr_t target, uint32_t image_size, const wchar_t*);
    bool (*native_capture)(const wchar_t*);
    int32_t (*frame_probe)(const wchar_t* library, const wchar_t* prefix);
    int32_t (*census)(const wchar_t* directory, uint32_t flags);
    bool (*camera_probe)(const wchar_t* directory);
    bool (*camera_reference)(const wchar_t* library);
    bool (*social_club)(bool trace);
} rsf_rdr2_asi_leaves;

#ifdef __cplusplus
extern "C" {
#endif
void rsf_rdr2_asi_process_attach(void* module, const rsf_rdr2_asi_leaves*,
    const rsf_rdr2_runtime_leaves*);
#ifdef __cplusplus
}
#endif
