// SPDX-License-Identifier: MIT
#pragma once
#include "sr_helpers.h"

// Private provider ABI. The native validators are shared with other SR callers.
using rsf_d3d12_sr_validate_resources = rsf_backend_result (*)(const rsf_sr_frame*, void*);
extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_plan(void*, rsf_quality, uint32_t*, uint32_t*);
extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_evaluate(void*, void*, const rsf_sr_frame*,
    rsf_d3d12_sr_validate_resources);
extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_release(void*);
extern "C" void rsf_core_d3d12_sr_provider_close(void*);
extern "C" rsf_backend_result rsf_core_d3d12_sr_provider_version(void*, uint64_t*, const char**);

inline rsf_backend_result rsf_d3d12_sr_provider_evaluate(void* session, void* commands, const rsf_sr_frame* frame)
{
    const auto result = rsf::validate_frame(frame);
    if (result != RSF_BACKEND_OK) return result;
    return rsf_core_d3d12_sr_provider_evaluate(session, commands, frame,
        [](const rsf_sr_frame* input, void* device) { return rsf::validate_d3d12_resources(*input, device); });
}
