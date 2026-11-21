// SPDX-License-Identifier: MIT
#pragma once
#include <stddef.h>
#include <stdint.h>

struct rsf_rdr2_d3d12_trace_hook {
    const void* detour;
    void** original;
    int32_t (*enable)(uintptr_t target);
};
static_assert(sizeof(rsf_rdr2_d3d12_trace_hook) == 24);

extern "C" int32_t rsf_rdr2_d3d12_trace_install(uintptr_t target, uint32_t image_size,
    const wchar_t* path, const rsf_rdr2_d3d12_trace_hook*) noexcept;
extern "C" void rsf_rdr2_d3d12_trace_record(int32_t result, uintptr_t base, uintptr_t caller,
    uint64_t tick, uint32_t thread, void* const* stack, size_t count) noexcept;
